/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Utilities for providing the hover feature
use crate::lsp::{HoverContents, LanguageString, MarkedString};
use crate::utils::{NodeKind, NodeResolutionInfo};
use graphql_ir::{Program, Value};
use graphql_text_printer::print_value;
use interner::StringKey;
use schema::Schema;
use schema_print::print_directive;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

fn graphql_marked_string(value: String) -> MarkedString {
    MarkedString::LanguageString(LanguageString {
        language: "graphql".to_string(),
        value,
    })
}

fn hover_content_wrapper(value: String) -> HoverContents {
    HoverContents::Scalar(graphql_marked_string(value))
}

/// This will provide a more accurate information about some of the specific Relay directives
/// that cannot be expressed via SDL
fn argument_definition_hover_info(directive_name: &str) -> Option<HoverContents> {
    let content = match directive_name {
        "argumentDefinitions" => Some(
            r#"
`@argumentDefinitions` is a directive used to specify arguments taken by a fragment.

---
@see: https://relay.dev/docs/en/graphql-in-relay.html#argumentdefinitions
"#,
        ),
        "arguments" => Some(
            r#"
`@arguments` is a directive used to pass arguments to a fragment that was defined using `@argumentDefinitions`.

---
@see: https://relay.dev/docs/en/graphql-in-relay.html#arguments
"#,
        ),
        "uncheckedArguments_DEPRECATED" => Some(
            r#"
DEPRECATED version of `@arguments` directive.
`@arguments` is a directive used to pass arguments to a fragment that was defined using `@argumentDefinitions`.

---
@see: https://relay.dev/docs/en/graphql-in-relay.html#arguments
"#,
        ),
        _ => None,
    };

    content.map(|value| HoverContents::Scalar(MarkedString::String(value.to_string())))
}

pub fn get_hover_response_contents(
    node_resolution_info: NodeResolutionInfo,
    schema: &Schema,
    source_programs: &Arc<RwLock<HashMap<StringKey, Program>>>,
) -> Option<HoverContents> {
    let kind = node_resolution_info.kind;

    match kind {
        NodeKind::Variable(type_name) => Some(hover_content_wrapper(type_name)),
        NodeKind::Directive(directive_name, argument_name) => {
            if let Some(argument_definition_hover_info) =
                argument_definition_hover_info(directive_name.lookup())
            {
                return Some(argument_definition_hover_info);
            }

            let schema_directive = schema.get_directive(directive_name)?;

            if let Some(argument_name) = argument_name {
                let argument = schema_directive.arguments.named(argument_name)?;
                let content = format!(
                    "{}: {}",
                    argument_name,
                    schema.get_type_string(&argument.type_)
                );
                Some(hover_content_wrapper(content))
            } else {
                let directive_text = print_directive(schema, &schema_directive);
                Some(hover_content_wrapper(directive_text))
            }
        }
        NodeKind::FieldName => {
            let type_ref = node_resolution_info
                .type_path
                .resolve_current_type_reference(schema)?;
            let type_name = schema.get_type_string(&type_ref);

            Some(hover_content_wrapper(type_name))
        }
        NodeKind::FieldArgument(field_name, argument_name) => {
            let type_ref = node_resolution_info
                .type_path
                .resolve_current_type_reference(schema)?;

            if type_ref.inner().is_object() || type_ref.inner().is_interface() {
                let field_id = schema.named_field(type_ref.inner(), field_name)?;
                let field = schema.field(field_id);
                let argument = field.arguments.named(argument_name)?;
                let content = format!(
                    "{}: {}",
                    argument_name,
                    schema.get_type_string(&argument.type_)
                );
                Some(hover_content_wrapper(content))
            } else {
                None
            }
        }
        NodeKind::FragmentSpread(fragment_name) => {
            let project_name = node_resolution_info.project_name;
            if let Some(source_program) = source_programs.read().unwrap().get(&project_name) {
                let fragment = source_program.fragment(fragment_name)?;
                let mut hover_contents: Vec<MarkedString> = vec![];
                hover_contents.push(graphql_marked_string(format!(
                    "fragment {} on {} {{ ... }}",
                    fragment.name.item,
                    schema.get_type_name(fragment.type_condition),
                )));

                if !fragment.variable_definitions.is_empty() {
                    let mut variables_string: Vec<String> =
                        vec!["This fragment accepts following arguments:".to_string()];
                    variables_string.push("```".to_string());
                    for var in &fragment.variable_definitions {
                        let default_value = match var.default_value.clone() {
                            Some(default_value) => {
                                format!(
                                    ", default_value = {}",
                                    print_value(schema, &Value::Constant(default_value))
                                )
                            }
                            None => "".to_string(),
                        };
                        variables_string.push(format!(
                            "- {}: {}{}",
                            var.name.item,
                            schema.get_type_string(&var.type_),
                            default_value,
                        ));
                    }
                    variables_string.push("```".to_string());
                    hover_contents.push(MarkedString::String(variables_string.join("\n")))
                }

                let fragment_name_details: Vec<&str> = fragment_name.lookup().split('_').collect();
                // We expect the fragment name to be `ComponentName_propName`
                if fragment_name_details.len() == 2 {
                    hover_contents.push(MarkedString::from_markdown(format!(
                        r#"
To consume this fragment spread,
pass it to the component that defined it.

For example:
```js
    <{} {}={{data.{}}} />
```
"#,
                        fragment_name_details[0],
                        fragment_name_details[1],
                        fragment_name_details[1],
                    )));
                } // We may log an error (later), if that is not the case.

                hover_contents.push(MarkedString::String(
                    "@see: https://relay.dev/docs/en/thinking-in-relay#data-masking".to_string(),
                ));
                Some(HoverContents::Array(hover_contents))
            } else {
                None
            }
        }
    }
}
