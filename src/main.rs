mod federation_gateway;
mod query_executor;
mod query_planner;
mod schema_registry;

use federation_gateway::FederationGateway;
use query_executor::HttpQueryExecutor;
use query_planner::SimpleQueryPlanner;
use schema_registry::InMemorySchemaRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

// Define our core types
type ServiceMap = HashMap<String, ServiceConfig>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ServiceConfig {
    name: String,
    url: String,
    schema: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct GraphQLRequest {
    query: String,
    variables: Option<Value>,
    operation_name: Option<String>,
    #[serde(skip)]
    auth_headers: Option<HashMap<String, String>>,
}

#[derive(Clone)]
struct FederatedSchema {
    services: ServiceMap,
    type_to_service_map: HashMap<String, Vec<String>>,
}

struct QueryPlan {
    service_queries: HashMap<String, String>,
    pub service_variables: HashMap<String, Value>,
}

// Create a response body from a string
fn full<T: Into<Bytes>>(value: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(value.into())
        .map_err(|never| match never {})
        .boxed()
}

// GraphiQL HTML template remains the same
const GRAPHIQL_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
  <title>GraphiQL - Portkey Federation Gateway</title>
  <link href="https://unpkg.com/graphiql@1.5.0/graphiql.min.css" rel="stylesheet" />
  <style>
    body { margin: 0; padding: 0; height: 100vh; }
    #graphiql { height: 100vh; }
  </style>
</head>
<body>
  <div id="graphiql"></div>

  <script src="https://unpkg.com/react@17.0.2/umd/react.production.min.js"></script>
  <script src="https://unpkg.com/react-dom@17.0.2/umd/react-dom.production.min.js"></script>
  <script src="https://unpkg.com/graphiql@1.5.0/graphiql.min.js"></script>
  <script>
   
    const token = localStorage.getItem('auth_token') || '';

   
    function graphQLFetcher(graphQLParams) {
      return fetch('/graphql', {
        method: 'post',
        headers: {
          'Content-Type': 'application/json',
         
          'Authorization': token ? `Bearer ${token}` : '',
        },
        body: JSON.stringify(graphQLParams),
      }).then(response => response.json());
    }

   
    ReactDOM.render(
      React.createElement(GraphiQL, { fetcher: graphQLFetcher }),
      document.getElementById('graphiql')
    );
  </script>
</body>
</html>
"#;

// Process incoming requests - unchanged
async fn handle_request(
    req: Request<Incoming>,
    gateway: Arc<FederationGateway>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
    let auth_headers = extract_auth_headers(&req);

    let result = match (req.method(), req.uri().path()) {
        (&Method::POST, "/graphql") => {
            let body_bytes = match req.collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(full("Failed to read request body"))
                        .unwrap());
                }
            };

            match serde_json::from_slice::<GraphQLRequest>(&body_bytes) {
                Ok(mut graphql_req) => {
                    graphql_req.auth_headers = auth_headers;

                    match gateway.process_request(graphql_req).await {
                        Ok(result) => {
                            let json = serde_json::to_string(&result).unwrap_or_default();
                            Response::builder()
                                .header("Content-Type", "application/json")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(full(json))
                                .unwrap_or_else(|_| internal_server_error())
                        }
                        Err(e) => {
                            let error_json = serde_json::to_string(&json!({
                                "errors": [{
                                    "message": e
                                }]
                            }))
                            .unwrap_or_default();

                            Response::builder()
                                .header("Content-Type", "application/json")
                                .header("Access-Control-Allow-Origin", "*")
                                .body(full(error_json))
                                .unwrap_or_else(|_| internal_server_error())
                        }
                    }
                }
                Err(e) => Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .header("Access-Control-Allow-Origin", "*")
                    .body(full(format!("Invalid JSON request: {}", e)))
                    .unwrap_or_else(|_| internal_server_error()),
            }
        }

        (&Method::GET, "/graphiql") => Response::builder()
            .header("Content-Type", "text/html")
            .header("Access-Control-Allow-Origin", "*")
            .body(full(GRAPHIQL_HTML))
            .unwrap_or_else(|_| internal_server_error()),

        (&Method::GET, "/") => Response::builder()
            .status(StatusCode::FOUND)
            .header("Location", "/graphiql")
            .header("Access-Control-Allow-Origin", "*")
            .body(full(""))
            .unwrap_or_else(|_| internal_server_error()),

        (&Method::OPTIONS, _) => Response::builder()
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
            .header(
                "Access-Control-Allow-Headers",
                "Content-Type, Authorization",
            )
            .body(full(""))
            .unwrap_or_else(|_| internal_server_error()),

        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Access-Control-Allow-Origin", "*")
            .body(full("Not Found"))
            .unwrap_or_else(|_| internal_server_error()),
    };

    Ok(result)
}

// Create a standard internal server error response
fn internal_server_error() -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(full("Internal Server Error"))
        .unwrap()
}

// Extract authentication headers from the request
fn extract_auth_headers(req: &Request<Incoming>) -> Option<HashMap<String, String>> {
    let mut auth_headers = HashMap::new();

    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            auth_headers.insert("Authorization".to_string(), auth_str.to_string());
        }
    }

    for header_name in ["x-api-key", "x-token"].iter() {
        if let Some(header_value) = req.headers().get(*header_name) {
            if let Ok(value_str) = header_value.to_str() {
                auth_headers.insert(header_name.to_string(), value_str.to_string());
            }
        }
    }

    if auth_headers.is_empty() {
        None
    } else {
        Some(auth_headers)
    }
}

#[derive(Clone)]
// An Executor that uses the tokio runtime.
pub struct TokioExecutor;

impl<F> hyper::rt::Executor<F> for TokioExecutor
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, fut: F) {
        tokio::task::spawn(fut);
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), std::boxed::Box<std::io::Error>> {
    let schema_registry = Box::new(InMemorySchemaRegistry::new());
    let query_planner = Box::new(SimpleQueryPlanner::new());
    let query_executor = Box::new(HttpQueryExecutor::new());

    let gateway = Arc::new(FederationGateway::new(
        schema_registry,
        query_planner,
        query_executor,
    ));

    if let Err(e) = gateway.load_schemas().await {
        eprintln!("Failed to load schemas: {}", e);
        return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, e)));
    }

    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 3000));

    let listener = TcpListener::bind(addr).await?;
    println!("GraphQL Federation Gateway starting on http://{}", addr);
    println!("GraphiQL UI available at http://{}/graphiql", addr);

    loop {
        let (stream, _addr) = listener.accept().await?;
        let io = TokioIo::new(stream);

        let gateway_clone = Arc::clone(&gateway);

        let executor = TokioExecutor;

        tokio::task::spawn(async move {
            let service = service_fn(move |req| {
                let gateway = gateway_clone.clone();
                handle_request(req, gateway)
            });

            match hyper_util::server::conn::auto::Builder::new(executor)
                .serve_connection(io, service)
                .await
            {
                Ok(_) => println!("Connection closed"),
                Err(e) => eprintln!("Error processing connection: {}", e),
            }
        });
    }
}
