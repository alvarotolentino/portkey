use async_trait::async_trait;
use graphql_parser::parse_schema;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{FederatedSchema, ServiceConfig, ServiceMap};

#[async_trait]
pub trait SchemaRegistry {
    async fn register_service(&mut self, service: ServiceConfig) -> Result<(), String>;
    async fn get_schema(&self) -> Result<FederatedSchema, String>;
    async fn refresh_schemas(&mut self) -> Result<(), String>;
}

// Implementation of our SchemaRegistry
pub struct InMemorySchemaRegistry {
    services: Arc<RwLock<ServiceMap>>,
    federated_schema: Arc<RwLock<Option<FederatedSchema>>>,
}

impl InMemorySchemaRegistry {
    pub fn new() -> Self {
        InMemorySchemaRegistry {
            services: Arc::new(RwLock::new(HashMap::new())),
            federated_schema: Arc::new(RwLock::new(None)),
        }
    }

    async fn build_federated_schema(
        &self,
        services: &ServiceMap,
    ) -> Result<FederatedSchema, String> {
        let mut type_to_service_map = HashMap::new();

        // Extract types from each schema and map them to their services
        for (service_name, service_config) in services {
            // Parse schema
            let schema_document = parse_schema::<String>(&service_config.schema).map_err(|e| {
                format!("Failed to parse schema for service {}: {}", service_name, e)
            })?;

            // Extract type names from schema
            for definition in &schema_document.definitions {
                if let graphql_parser::schema::Definition::TypeDefinition(typedef) = definition {
                    let type_name = match typedef {
                        graphql_parser::schema::TypeDefinition::Object(obj) => obj.name.clone(),
                        graphql_parser::schema::TypeDefinition::Interface(iface) => {
                            iface.name.clone()
                        }
                        graphql_parser::schema::TypeDefinition::InputObject(input) => {
                            input.name.clone()
                        }
                        graphql_parser::schema::TypeDefinition::Enum(enum_type) => {
                            enum_type.name.clone()
                        }
                        graphql_parser::schema::TypeDefinition::Scalar(scalar) => {
                            scalar.name.clone()
                        }
                        graphql_parser::schema::TypeDefinition::Union(union_type) => {
                            union_type.name.clone()
                        }
                    };

                    // Add this service to the list of services that provide this type
                    type_to_service_map
                        .entry(type_name)
                        .or_insert_with(Vec::new)
                        .push(service_name.clone());
                }
            }
        }

        Ok(FederatedSchema {
            services: services.clone(),
            type_to_service_map,
        })
    }
}

#[async_trait]
impl SchemaRegistry for InMemorySchemaRegistry {
    async fn register_service(&mut self, service: ServiceConfig) -> Result<(), String> {
        let mut services = self.services.write().await;
        services.insert(service.name.clone(), service);

        // Invalidate cached schema
        let mut federated_schema = self.federated_schema.write().await;
        *federated_schema = None;

        Ok(())
    }

    async fn get_schema(&self) -> Result<FederatedSchema, String> {
        // Check if we have a cached schema
        let cached_schema = self.federated_schema.read().await;
        if let Some(schema) = &*cached_schema {
            return Ok(schema.clone());
        }
        drop(cached_schema);

        // Build a new schema
        let services = self.services.read().await;
        let schema = self.build_federated_schema(&services).await?;

        // Cache the schema
        let mut federated_schema = self.federated_schema.write().await;
        *federated_schema = Some(schema.clone());

        Ok(schema)
    }

    async fn refresh_schemas(&mut self) -> Result<(), String> {
        // Invalidate cached schema to force rebuild on next get_schema call
        let mut federated_schema = self.federated_schema.write().await;
        *federated_schema = None;
        Ok(())
    }
}
