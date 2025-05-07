pub mod federation_gateway;
pub mod query_executor;
pub mod query_planner;
pub mod schema_registry;

pub use federation_gateway::FederationGateway;
pub use query_executor::HttpQueryExecutor;
pub use query_planner::SimpleQueryPlanner;
pub use schema_registry::InMemorySchemaRegistry;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

type ServiceMap = HashMap<String, ServiceConfig>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    pub url: String,
    pub schema: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GraphQLRequest {
    pub query: String,
    pub variables: Option<Value>,
    pub operation_name: Option<String>,
    #[serde(skip)]
    pub auth_headers: Option<HashMap<String, String>>,
}

#[derive(Clone)]
pub struct FederatedSchema {
    pub services: ServiceMap,
    pub type_to_service_map: HashMap<String, Vec<String>>,
}

pub struct QueryPlan {
    pub service_queries: HashMap<String, String>,
    pub service_variables: HashMap<String, Value>,
}
