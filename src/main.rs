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
use std::sync::Arc;
use warp::Filter;

// Core types for our federation system
type SchemaMap = HashMap<String, String>;
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize our components
    let schema_registry = Box::new(InMemorySchemaRegistry::new());
    let query_planner = Box::new(SimpleQueryPlanner::new());
    let query_executor = Box::new(HttpQueryExecutor::new());

    // Create our gateway
    let gateway = Arc::new(FederationGateway::new(
        schema_registry,
        query_planner,
        query_executor,
    ));

    // Register schemas
    gateway.load_schemas().await?;

    // Create a Warp filter for processing GraphQL requests
    let gateway_clone = gateway.clone();
    let graphql_route = warp::path("graphql")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |request: GraphQLRequest| {
            let gateway = gateway_clone.clone();
            async move {
                match gateway.process_request(request).await {
                    Ok(response) => Ok::<_, warp::Rejection>(warp::reply::json(&response)),
                    Err(err) => {
                        let error_response = json!({
                            "errors": [{
                                "message": err
                            }]
                        });
                        Ok(warp::reply::json(&error_response))
                    }
                }
            }
        });

    // Create a route for service registration
    let gateway_clone = gateway.clone();
    let register_route = warp::path("register")
        .and(warp::post())
        .and(warp::body::json())
        .and_then(move |service: ServiceConfig| {
            let gateway = gateway_clone.clone();
            async move {
                match gateway.register_service(service).await {
                    Ok(_) => Ok::<_, warp::Rejection>(warp::reply::json(&json!({"success": true}))),
                    Err(err) => {
                        let error_response = json!({
                            "success": false,
                            "error": err
                        });
                        Ok(warp::reply::json(&error_response))
                    }
                }
            }
        });

    // Combine routes and start the server
    let routes = graphql_route.or(register_route);

    println!("GraphQL Federation Gateway starting on port 3000");
    warp::serve(routes).run(([127, 0, 0, 1], 3000)).await;

    Ok(())
}
