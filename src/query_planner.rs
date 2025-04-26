use async_trait::async_trait;
use graphql_parser::query::parse_query;
use std::collections::HashMap;

use crate::{FederatedSchema, QueryPlan};

#[async_trait]
pub trait QueryPlanner {
    async fn plan_query(&self, query: &str, schema: &FederatedSchema) -> Result<QueryPlan, String>;
}

pub struct SimpleQueryPlanner;

impl SimpleQueryPlanner {
    pub fn new() -> Self {
        SimpleQueryPlanner
    }

    fn extract_operation_types(&self, query: &str) -> Result<Vec<String>, String> {
        let query_document =
            parse_query::<String>(query).map_err(|e| format!("Failed to parse query: {}", e))?;

        let mut types = Vec::new();

        // Extract root operation types from the query
        for definition in &query_document.definitions {
            if let graphql_parser::query::Definition::Operation(op) = definition {
                match op {
                    graphql_parser::query::OperationDefinition::Query(q) => {
                        // Extract selection types from query
                        for selection in &q.selection_set.items {
                            if let graphql_parser::query::Selection::Field(field) = selection {
                                types.push(field.name.clone());
                            }
                        }
                    }
                    graphql_parser::query::OperationDefinition::Mutation(m) => {
                        // Extract selection types from mutation
                        for selection in &m.selection_set.items {
                            if let graphql_parser::query::Selection::Field(field) = selection {
                                types.push(field.name.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(types)
    }

    fn find_service_for_operation(
        &self,
        operation_type: &str,
        schema: &FederatedSchema,
    ) -> Option<String> {
        // First try an exact match on the operation name
        if let Some(services) = schema.type_to_service_map.get(operation_type) {
            if !services.is_empty() {
                return Some(services[0].clone());
            }
        }

        // If no exact match, we could implement more sophisticated routing here
        // For now, returning None indicates we don't know where to route this
        None
    }
}

#[async_trait]
impl QueryPlanner for SimpleQueryPlanner {
    async fn plan_query(&self, query: &str, schema: &FederatedSchema) -> Result<QueryPlan, String> {
        let operation_types = self.extract_operation_types(query)?;
        let mut service_queries = HashMap::new();

        // For each operation, find the appropriate service
        for op_type in operation_types {
            if let Some(service_name) = self.find_service_for_operation(&op_type, schema) {
                // For simplicity, we're forwarding the entire query to each service
                // In a real implementation, you'd split the query and only send relevant parts
                service_queries.insert(service_name, query.to_string());
            } else {
                return Err(format!("No service found for operation type: {}", op_type));
            }
        }

        if service_queries.is_empty() {
            return Err("Could not route query to any service".to_string());
        }

        Ok(QueryPlan { service_queries })
    }
}
