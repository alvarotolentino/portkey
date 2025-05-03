mod federation_gateway;
mod query_executor;
mod query_planner;
mod schema_registry;

use actix_cors::Cors;
use actix_web::{App, HttpResponse, HttpServer, Responder, middleware, web};
use federation_gateway::FederationGateway;
use query_executor::HttpQueryExecutor;
use query_planner::SimpleQueryPlanner;
use schema_registry::InMemorySchemaRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

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

fn cors_config() -> Cors {
    Cors::default()
        .allow_any_origin()
        .allowed_methods(vec!["GET", "POST", "OPTIONS"])
        .allowed_headers(vec![
            "Content-Type",
            "Authorization",
            "Apollo-Federation-Include-Trace",
        ])
        .supports_credentials()
}

// Gateway handler for GraphQL requests
async fn graphql_handler(
    gateway: web::Data<Arc<FederationGateway>>,
    request: web::Json<GraphQLRequest>,
) -> impl Responder {
    match gateway.process_request(request.into_inner()).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(err) => {
            let error_response = json!({
                "errors": [{
                    "message": err
                }]
            });
            HttpResponse::Ok().json(error_response)
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
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
        return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
    }

    let gateway_data = web::Data::new(gateway);

    println!("GraphQL Federation Gateway starting on port 3000");

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(cors_config())
            .app_data(gateway_data.clone())
            .route("/graphql", web::post().to(graphql_handler))
    })
    .bind(("127.0.0.1", 3000))?
    .run()
    .await
}
