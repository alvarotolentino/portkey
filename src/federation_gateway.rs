use serde::Deserialize;
use serde_json::Value;
use std::{collections::HashMap, fs, io, path::Path, sync::Arc};
use tokio::sync::RwLock;

use crate::{
    GraphQLRequest, ServiceConfig, query_executor::QueryExecutor, query_planner::QueryPlanner,
    schema_registry::SchemaRegistry,
};

#[derive(Debug, Deserialize)]
struct SupergraphConfig {
    subgraphs: HashMap<String, SubgraphConfig>,
}

#[derive(Debug, Deserialize)]
struct SubgraphConfig {
    routing_url: String,
    schema: SchemaConfig,
}

#[derive(Debug, Deserialize)]
struct SchemaConfig {
    file: String,
}
pub struct FederationGateway {
    schema_registry: Arc<RwLock<Box<dyn SchemaRegistry + Send + Sync>>>,
    query_planner: Arc<Box<dyn QueryPlanner + Send + Sync>>,
    query_executor: Arc<Box<dyn QueryExecutor + Send + Sync>>,
}

impl FederationGateway {
    pub fn new(
        schema_registry: Box<dyn SchemaRegistry + Send + Sync>,
        query_planner: Box<dyn QueryPlanner + Send + Sync>,
        query_executor: Box<dyn QueryExecutor + Send + Sync>,
    ) -> Self {
        FederationGateway {
            schema_registry: Arc::new(RwLock::new(schema_registry)),
            query_planner: Arc::new(query_planner),
            query_executor: Arc::new(query_executor),
        }
    }

    pub async fn process_request(&self, request: GraphQLRequest) -> Result<Value, String> {
        println!("Processing request: {:?}", request);

        let schema_registry = self.schema_registry.read().await;
        let schema = schema_registry.get_schema().await?;
        drop(schema_registry);

        let query_plan = self
            .query_planner
            .plan_query(&request.query, &schema, request.variables)
            .await?;

        let response = self
            .query_executor
            .execute_plan(query_plan, &schema)
            .await?;

        Ok(response)
    }

    pub async fn register_service(&self, service: ServiceConfig) -> Result<(), String> {
        let mut schema_registry = self.schema_registry.write().await;
        schema_registry.register_service(service).await
    }

    pub async fn load_schemas(&self) -> Result<(), String> {
        let config_path = Path::new("./schemas/supergraph.yaml");
        let config_dir = config_path.parent().unwrap_or_else(|| Path::new(""));
        println!("Config path: {:?}", config_path);

        let config_contents = fs::read_to_string(config_path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let config: SupergraphConfig = serde_yaml::from_str(&config_contents)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        for (name, subgraph_config) in config.subgraphs {
            let schema_content = read_schema_file(config_dir, &subgraph_config.schema.file)
                .map_err(|e| format!("Failed to read schema file: {}", e))?;

            let service_config = ServiceConfig {
                name,
                url: subgraph_config.routing_url,
                schema: schema_content,
            };

            self.register_service(service_config).await?;
        }
        Ok(())
    }
}

fn read_schema_file(base_dir: &Path, file_path: &str) -> io::Result<String> {
    let full_path = base_dir.join(file_path);
    println!("Reading schema file: {:?}", full_path);
    fs::read_to_string(full_path)
}
