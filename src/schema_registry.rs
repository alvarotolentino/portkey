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
}

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

        for (service_name, service_config) in services {
            let schema_document = parse_schema::<String>(&service_config.schema).map_err(|e| {
                format!("Failed to parse schema for service {}: {}", service_name, e)
            })?;

            for definition in &schema_document.definitions {
                if let graphql_parser::schema::Definition::TypeDefinition(typedef) = definition {
                    match typedef {
                        graphql_parser::schema::TypeDefinition::Object(obj) => {
                            let type_name = obj.name.clone();
                            type_to_service_map
                                .entry(type_name.clone())
                                .or_insert_with(Vec::new)
                                .push(service_name.clone());

                            for field in &obj.fields {
                                let field_key = format!("{}.{}", type_name, field.name);
                                type_to_service_map
                                    .entry(field_key)
                                    .or_insert_with(Vec::new)
                                    .push(service_name.clone());

                                for arg in &field.arguments {
                                    let arg_key =
                                        format!("{}.{}.{}", type_name, field.name, arg.name);
                                    type_to_service_map
                                        .entry(arg_key)
                                        .or_insert_with(Vec::new)
                                        .push(service_name.clone());
                                }
                            }
                        }
                        graphql_parser::schema::TypeDefinition::Interface(iface) => {
                            let type_name = iface.name.clone();
                            type_to_service_map
                                .entry(type_name)
                                .or_insert_with(Vec::new)
                                .push(service_name.clone());
                        }
                        graphql_parser::schema::TypeDefinition::InputObject(input) => {
                            let type_name = input.name.clone();
                            type_to_service_map
                                .entry(type_name)
                                .or_insert_with(Vec::new)
                                .push(service_name.clone());
                        }
                        graphql_parser::schema::TypeDefinition::Enum(enum_type) => {
                            let type_name = enum_type.name.clone();
                            type_to_service_map
                                .entry(type_name)
                                .or_insert_with(Vec::new)
                                .push(service_name.clone());
                        }
                        graphql_parser::schema::TypeDefinition::Scalar(scalar) => {
                            let type_name = scalar.name.clone();
                            type_to_service_map
                                .entry(type_name)
                                .or_insert_with(Vec::new)
                                .push(service_name.clone());
                        }
                        graphql_parser::schema::TypeDefinition::Union(union_type) => {
                            let type_name = union_type.name.clone();
                            type_to_service_map
                                .entry(type_name)
                                .or_insert_with(Vec::new)
                                .push(service_name.clone());
                        }
                    }
                }
            }
        }

        println!("Type to service map: {:?}", type_to_service_map);
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

        let mut federated_schema = self.federated_schema.write().await;
        *federated_schema = None;

        Ok(())
    }

    async fn get_schema(&self) -> Result<FederatedSchema, String> {
        let cached_schema = self.federated_schema.read().await;
        if let Some(schema) = &*cached_schema {
            return Ok(schema.clone());
        }
        drop(cached_schema);

        let services = self.services.read().await;
        let schema = self.build_federated_schema(&services).await?;

        let mut federated_schema = self.federated_schema.write().await;
        *federated_schema = Some(schema.clone());

        Ok(schema)
    }
}
