use async_trait::async_trait;
use graphql_parser::query::{
    self, Definition, OperationDefinition, SelectionSet, VariableDefinition,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::{FederatedSchema, QueryPlan};

#[async_trait]
pub trait QueryPlanner: Send + Sync {
    async fn plan_query(
        &self,
        query: &str,
        schema: &FederatedSchema,
        variables: Option<Value>,
    ) -> Result<QueryPlan, String>;
}

pub struct SimpleQueryPlanner {}

impl SimpleQueryPlanner {
    pub fn new() -> Self {
        SimpleQueryPlanner {}
    }

    fn find_variables_in_field(field: &query::Field<String>) -> HashSet<String> {
        let mut variables = HashSet::new();
        Self::collect_variables_from_field(field, &mut variables);
        variables
    }

    fn collect_variables_from_field(field: &query::Field<String>, variables: &mut HashSet<String>) {
        for (_, value) in &field.arguments {
            Self::extract_variables_from_value(value, variables);
        }

        for selection in &field.selection_set.items {
            if let query::Selection::Field(nested_field) = selection {
                Self::collect_variables_from_field(nested_field, variables);
            }
        }
    }

    fn extract_variables_from_value(value: &query::Value<String>, variables: &mut HashSet<String>) {
        match value {
            query::Value::Variable(var_name) => {
                variables.insert(var_name.clone());
            }
            query::Value::List(items) => {
                for item in items {
                    Self::extract_variables_from_value(item, variables);
                }
            }
            query::Value::Object(obj) => {
                for val in obj.values() {
                    Self::extract_variables_from_value(val, variables);
                }
            }
            _ => {}
        }
    }

    fn extract_fields<'a>(
        selection_set: &'a SelectionSet<'a, String>,
    ) -> impl Iterator<Item = &'a query::Field<'a, String>> + 'a {
        selection_set.items.iter().filter_map(|selection| {
            if let query::Selection::Field(field) = selection {
                Some(field)
            } else {
                None
            }
        })
    }

    fn find_service_for_field(
        field_name: &str,
        operation_type: &str,
        schema: &FederatedSchema,
    ) -> Result<String, String> {
        let type_key = format!("{}.{}", operation_type, field_name);

        if let Some((_, service_names)) = schema.type_to_service_map.get_key_value(&type_key) {
            if !service_names.is_empty() {
                return Ok(service_names[0].clone());
            }
        }

        Err(format!(
            "No service found for field: {} in operation: {}",
            field_name, operation_type
        ))
    }

    fn create_field_query(
        field: &query::Field<String>,
        operation_type: &str,
        variable_defs: &[VariableDefinition<String>],
        used_variables: &HashSet<String>,
    ) -> String {
        let estimated_size = 100
            + field.name.len() * 2
            + used_variables.len() * 20
            + field.selection_set.items.len() * 30;

        let mut query_str = String::with_capacity(estimated_size);

        match operation_type {
            "Query" => query_str.push_str("query"),
            "Mutation" => query_str.push_str("mutation"),
            "Subscription" => query_str.push_str("subscription"),
            _ => query_str.push_str("query"),
        }

        if !used_variables.is_empty() {
            query_str.push('(');

            let mut first = true;
            for def in variable_defs {
                if used_variables.contains(&def.name) {
                    if !first {
                        query_str.push_str(", ");
                    }
                    first = false;

                    write!(query_str, "${}: {}", def.name, def.var_type).unwrap();

                    if let Some(default_value) = &def.default_value {
                        query_str.push_str(" = ");
                        Self::append_value(&mut query_str, default_value);
                    }
                }
            }

            query_str.push(')');
        }

        query_str.push_str(" {\n  ");
        query_str.push_str(&field.name);

        if !field.arguments.is_empty() {
            query_str.push('(');

            let mut first = true;
            for (name, value) in &field.arguments {
                if !first {
                    query_str.push_str(", ");
                }
                first = false;

                query_str.push_str(name);
                query_str.push_str(": ");
                Self::append_value(&mut query_str, value);
            }

            query_str.push(')');
        }

        if !field.selection_set.items.is_empty() {
            query_str.push_str(" {\n");
            Self::append_selection_set(&mut query_str, &field.selection_set, 4);
            query_str.push_str("  }\n");
        }

        query_str.push_str("}\n");
        query_str
    }

    fn append_value(out: &mut String, value: &query::Value<String>) {
        match value {
            query::Value::Variable(var_name) => {
                out.push('$');
                out.push_str(var_name);
            }
            query::Value::String(s) => {
                out.push('"');

                for c in s.chars() {
                    if c == '"' {
                        out.push('\\');
                    }
                    out.push(c);
                }
                out.push('"');
            }
            query::Value::Int(i) => {
                write!(out, "{:?}", i).unwrap();
            }
            query::Value::Float(f) => {
                write!(out, "{}", f).unwrap();
            }
            query::Value::Boolean(b) => {
                out.push_str(if *b { "true" } else { "false" });
            }
            query::Value::Null => {
                out.push_str("null");
            }
            query::Value::Enum(e) => {
                out.push_str(e);
            }
            query::Value::List(l) => {
                out.push('[');
                let mut first = true;
                for item in l {
                    if !first {
                        out.push_str(", ");
                    }
                    first = false;
                    Self::append_value(out, item);
                }
                out.push(']');
            }
            query::Value::Object(o) => {
                out.push('{');
                let mut first = true;
                for (k, v) in o {
                    if !first {
                        out.push_str(", ");
                    }
                    first = false;
                    out.push_str(k);
                    out.push_str(": ");
                    Self::append_value(out, v);
                }
                out.push('}');
            }
        }
    }

    fn append_selection_set(
        query_str: &mut String,
        selection_set: &SelectionSet<String>,
        indent: usize,
    ) {
        let indent_str = " ".repeat(indent);

        for selection in &selection_set.items {
            match selection {
                query::Selection::Field(field) => {
                    query_str.push_str(&indent_str);
                    query_str.push_str(&field.name);

                    if !field.arguments.is_empty() {
                        query_str.push('(');
                        let mut first = true;
                        for (name, value) in &field.arguments {
                            if !first {
                                query_str.push_str(", ");
                            }
                            first = false;

                            query_str.push_str(name);
                            query_str.push_str(": ");
                            Self::append_value(query_str, value);
                        }
                        query_str.push(')');
                    }

                    if !field.selection_set.items.is_empty() {
                        query_str.push_str(" {\n");
                        Self::append_selection_set(query_str, &field.selection_set, indent + 2);
                        query_str.push_str(&indent_str);
                        query_str.push_str("}\n");
                    } else {
                        query_str.push('\n');
                    }
                }
                query::Selection::FragmentSpread(fragment) => {
                    query_str.push_str(&indent_str);
                    query_str.push_str("...");
                    query_str.push_str(&fragment.fragment_name);
                    query_str.push('\n');
                }
                query::Selection::InlineFragment(fragment) => {
                    query_str.push_str(&indent_str);
                    query_str.push_str("... ");

                    if let Some(type_condition) = &fragment.type_condition {
                        query_str.push_str("on ");
                        query_str.push_str(&type_condition.to_string());
                        query_str.push(' ');
                    }

                    query_str.push_str("{\n");
                    Self::append_selection_set(query_str, &fragment.selection_set, indent + 2);
                    query_str.push_str(&indent_str);
                    query_str.push_str("}\n");
                }
            }
        }
    }
}

#[async_trait]
impl QueryPlanner for SimpleQueryPlanner {
    async fn plan_query(
        &self,
        query: &str,
        schema: &FederatedSchema,
        variables: Option<Value>,
    ) -> Result<QueryPlan, String> {
        let doc = match graphql_parser::query::parse_query::<String>(query) {
            Ok(doc) => doc,
            Err(e) => return Err(format!("Failed to parse query: {}", e)),
        };

        let mut service_queries = HashMap::with_capacity(4);
        let mut service_variables = HashMap::with_capacity(4);

        for def in &doc.definitions {
            match def {
                Definition::Operation(OperationDefinition::SelectionSet(selection_set)) => {
                    for field in Self::extract_fields(selection_set) {
                        let service_name =
                            Self::find_service_for_field(&field.name, "Query", schema)?;
                        let field_variables = Self::find_variables_in_field(field);

                        let field_query =
                            Self::create_field_query(field, "Query", &[], &field_variables);
                        service_queries.insert(service_name.clone(), field_query);

                        if let Some(var_values) = &variables {
                            if field_variables.is_empty() {
                                service_variables.insert(service_name, json!({}));
                            } else if let Value::Object(obj) = var_values {
                                let mut field_vars =
                                    serde_json::Map::with_capacity(field_variables.len());

                                for var_name in &field_variables {
                                    if let Some(var_value) = obj.get(var_name) {
                                        field_vars.insert(var_name.clone(), var_value.clone());
                                    }
                                }

                                service_variables.insert(service_name, Value::Object(field_vars));
                            } else {
                                service_variables.insert(service_name, json!({}));
                            }
                        } else {
                            service_variables.insert(service_name, json!({}));
                        }
                    }
                }

                Definition::Operation(op) => {
                    let (operation_type, selection_set, var_defs) = match op {
                        OperationDefinition::Query(q) => {
                            ("Query", &q.selection_set, &q.variable_definitions[..])
                        }
                        OperationDefinition::Mutation(m) => {
                            ("Mutation", &m.selection_set, &m.variable_definitions[..])
                        }
                        OperationDefinition::Subscription(s) => (
                            "Subscription",
                            &s.selection_set,
                            &s.variable_definitions[..],
                        ),
                        _ => continue,
                    };

                    for field in Self::extract_fields(selection_set) {
                        let service_name =
                            Self::find_service_for_field(&field.name, operation_type, schema)?;
                        let field_variables = Self::find_variables_in_field(field);

                        let field_query = Self::create_field_query(
                            field,
                            operation_type,
                            var_defs,
                            &field_variables,
                        );
                        service_queries.insert(service_name.clone(), field_query);

                        if let Some(var_values) = &variables {
                            if field_variables.is_empty() {
                                service_variables.insert(service_name, json!({}));
                                continue;
                            }

                            if let Value::Object(obj) = var_values {
                                let mut field_vars =
                                    serde_json::Map::with_capacity(field_variables.len());

                                for var_name in &field_variables {
                                    if let Some(var_value) = obj.get(var_name) {
                                        field_vars.insert(var_name.clone(), var_value.clone());
                                    }
                                }

                                service_variables.insert(service_name, Value::Object(field_vars));
                            } else {
                                service_variables.insert(service_name, json!({}));
                            }
                        } else {
                            service_variables.insert(service_name, json!({}));
                        }
                    }
                }
                _ => continue,
            }
        }

        if service_queries.is_empty() {
            return Err("No valid operations found in query".to_string());
        }

        #[cfg(debug_assertions)]
        {
            println!("Generated service queries: {:?}", service_queries);
            println!("Variable distribution: {:?}", service_variables);
        }

        Ok(QueryPlan {
            service_queries,
            service_variables,
        })
    }
}
