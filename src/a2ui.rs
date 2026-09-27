//! A2UI v1.0 envelope handling for the `render_ui` MCP tool.
//!
//! The sidebar renderer is deliberately forgiving — a surface it can't fully
//! understand still paints whatever it can. That is the wrong contract for the
//! agent: a dangling child reference or a mistyped component name produces a
//! blank card and a confused model that has no idea why. So every envelope is
//! checked here first, and a broken one comes back as a tool error naming the
//! exact problem instead of rendering.
//!
//! Spec: <https://a2ui.org/specification/v1.0-a2ui/>

use serde_json::Value;

/// Stamped on every message the agent sends us.
pub const PROTOCOL_VERSION: &str = "v1.0";

/// Surface default when the agent doesn't name a catalog.
pub const BASIC_CATALOG_ID: &str =
    "https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json";

/// The v1.0 basic catalog, in catalog order.
pub const CATALOG_COMPONENTS: &[&str] = &[
    "AudioPlayer",
    "Button",
    "Card",
    "CheckBox",
    "ChoicePicker",
    "Column",
    "DateTimeInput",
    "Divider",
    "Icon",
    "Image",
    "List",
    "Modal",
    "Row",
    "Slider",
    "Tabs",
    "Text",
    "TextField",
    "Video",
];

/// Components the renderer still draws but the catalog never had. They predate
/// the v1.0 upgrade and stay accepted so surfaces replayed from an older
/// session keep working; `render_ui`'s description no longer offers them.
const LEGACY_COMPONENTS: &[&str] = &["Heading", "Markdown", "Spacer"];

/// The six agent → renderer message kinds.
const MESSAGE_KINDS: &[&str] = &[
    "createSurface",
    "updateComponents",
    "updateDataModel",
    "deleteSurface",
    "callRendererFunction",
    "agentFunctionResponse",
];

/// Props that name another component by id.
const CHILD_PROPS: &[&str] = &["child", "trigger", "content"];

fn is_known_component(name: &str) -> bool {
    CATALOG_COMPONENTS.contains(&name) || LEGACY_COMPONENTS.contains(&name)
}

/// Mirrors the sidebar renderer's Button: a click needs an event, an `openUrl`
/// or a `functionCall` to go anywhere, and without one the renderer draws a
/// broken control. `actions` is a spelling agents emit that it also accepts.
fn button_action(button: &serde_json::Map<String, Value>) -> Option<&Value> {
    button.get("action").or_else(|| button.get("actions"))
}

fn has_button_action(button: &serde_json::Map<String, Value>) -> bool {
    let Some(action) = button_action(button) else {
        return false;
    };
    action
        .pointer("/event/name")
        .and_then(Value::as_str)
        .is_some_and(|name| !name.is_empty())
        || action
            .pointer("/functionCall/call")
            .is_some_and(Value::is_string)
        || action
            .get("openUrl")
            .is_some_and(|url| url.is_string() || url.pointer("/url").is_some_and(Value::is_string))
}

/// Why a blocking `render_ui` on a new surface could never be answered: it
/// waits on `await_events`, but no Button in the envelope sends an event. Any
/// event click completes the call, so only a surface with none is stuck until
/// the timeout. Incremental envelopes may be answered by Buttons an earlier
/// call drew, and a `callRendererFunction` answers the call itself.
pub fn unanswerable_wait(messages: &[Value], await_events: &[String]) -> Option<String> {
    let creates_surface = messages.iter().any(|m| m.get("createSurface").is_some());
    let calls_renderer = messages
        .iter()
        .any(|m| m.get("callRendererFunction").is_some());
    if await_events.is_empty() || !creates_surface || calls_renderer {
        return None;
    }
    let sends_event = messages
        .iter()
        .filter_map(|m| m.get("createSurface").or_else(|| m.get("updateComponents")))
        .filter_map(|body| body.get("components")?.as_array())
        .flatten()
        .filter_map(Value::as_object)
        .filter(|c| c.get("component").and_then(Value::as_str) == Some("Button"))
        .filter_map(button_action)
        .any(|action| {
            action
                .pointer("/event/name")
                .and_then(Value::as_str)
                .is_some_and(|n| !n.is_empty())
        });
    (!sends_event).then(|| {
        format!(
            "await_events is {await_events:?}, but no Button on this surface has an \
             \"action\": {{\"event\": {{\"name\"}}}}, so nothing can answer and the call would \
             wait until it times out. Give a Button one of those event names, or drop \
             await_events for a fire-and-forget update."
        )
    })
}

/// The message kind of `msg`, ignoring the `version` envelope field.
fn kinds_of(msg: &serde_json::Map<String, Value>) -> Vec<&'static str> {
    MESSAGE_KINDS
        .iter()
        .copied()
        .filter(|k| msg.contains_key(*k))
        .collect()
}

/// Bring an envelope up to v1.0 shape: stamp the protocol version on every
/// message and give a surface the basic catalog when it names none. Both are
/// required by the schema but neither changes what gets drawn, so we fill them
/// in rather than rejecting an envelope over them.
/// Fill a missing `surfaceId` on follow-up messages from the one `createSurface`
/// in the same envelope. Models routinely open a surface and update it in one
/// call, naming the surface only once; the intent is unambiguous when exactly
/// one surface is created, and bouncing the envelope taught them to give up on
/// surfaces altogether. Runs before `validate`.
pub fn fill_surface_ids(messages: &mut [Value]) {
    let created: Vec<&str> = messages
        .iter()
        .filter_map(|m| m.get("createSurface")?.get("surfaceId")?.as_str())
        .collect();
    let [id] = created.as_slice() else {
        return;
    };
    let id = Value::String((*id).to_owned());
    for msg in messages.iter_mut() {
        for kind in ["updateComponents", "updateDataModel", "deleteSurface"] {
            let Some(body) = msg.get_mut(kind).and_then(|b| b.as_object_mut()) else {
                continue;
            };
            let missing = !body
                .get("surfaceId")
                .and_then(|s| s.as_str())
                .is_some_and(|s| !s.is_empty());
            if missing {
                body.insert("surfaceId".into(), id.clone());
            }
        }
    }
}

pub fn normalize(messages: &mut [Value]) {
    for msg in messages.iter_mut() {
        let Some(obj) = msg.as_object_mut() else {
            continue;
        };
        obj.insert("version".into(), Value::String(PROTOCOL_VERSION.into()));
        if let Some(surface) = obj.get_mut("createSurface").and_then(|s| s.as_object_mut()) {
            surface
                .entry("catalogId")
                .or_insert_with(|| Value::String(BASIC_CATALOG_ID.into()));
        }
    }
}

/// Everything wrong with this envelope, in the order it was found. Empty means
/// the renderer can draw it.
pub fn validate(messages: &[Value]) -> Vec<String> {
    let mut problems = Vec::new();
    // Component ids declared anywhere in this envelope, and the references
    // they make — checked together at the end, once every message is seen.
    let mut declared: Vec<String> = Vec::new();
    let mut referenced: Vec<(String, String)> = Vec::new();
    // Buttons with nowhere for a click to go, as (path, id).
    let mut actionless: Vec<(String, String)> = Vec::new();
    let mut has_create_surface = false;

    for (i, msg) in messages.iter().enumerate() {
        let at = format!("messages[{i}]");
        let Some(obj) = msg.as_object() else {
            problems.push(format!("{at} is not a JSON object."));
            continue;
        };
        if let Some(v) = obj.get("version").and_then(|v| v.as_str()) {
            if v != PROTOCOL_VERSION {
                problems.push(format!(
                    "{at}.version is \"{v}\" — this renderer speaks A2UI {PROTOCOL_VERSION}."
                ));
            }
        }
        let kinds = kinds_of(obj);
        let kind = match kinds.as_slice() {
            [one] => *one,
            [] => {
                problems.push(format!(
                    "{at} has no message kind. Each message is exactly one of: {}.",
                    MESSAGE_KINDS.join(", ")
                ));
                continue;
            }
            many => {
                problems.push(format!(
                    "{at} carries {} message kinds ({}) — send one per message.",
                    many.len(),
                    many.join(", ")
                ));
                continue;
            }
        };
        let body = &obj[kind];
        let Some(body) = body.as_object() else {
            problems.push(format!("{at}.{kind} is not a JSON object."));
            continue;
        };

        if matches!(
            kind,
            "createSurface" | "updateComponents" | "updateDataModel" | "deleteSurface"
        ) && !body
            .get("surfaceId")
            .and_then(|s| s.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            problems.push(format!("{at}.{kind} needs a non-empty \"surfaceId\"."));
        }

        match kind {
            "createSurface" => {
                has_create_surface = true;
                if let Some(components) = body.get("components") {
                    check_components(
                        components,
                        &format!("{at}.createSurface.components"),
                        &mut problems,
                        &mut declared,
                        &mut referenced,
                        &mut actionless,
                    );
                }
                if body.get("dataModel").is_some_and(|d| !d.is_object()) {
                    problems.push(format!("{at}.createSurface.dataModel must be an object."));
                }
            }
            "updateComponents" => match body.get("components") {
                Some(components) => check_components(
                    components,
                    &format!("{at}.updateComponents.components"),
                    &mut problems,
                    &mut declared,
                    &mut referenced,
                    &mut actionless,
                ),
                None => problems.push(format!(
                    "{at}.updateComponents needs a \"components\" array."
                )),
            },
            "updateDataModel" => {
                if !body.contains_key("value") {
                    problems.push(format!(
                        "{at}.updateDataModel needs a \"value\" (use null to delete the key at \"path\")."
                    ));
                }
                if let Some(path) = body.get("path").and_then(|p| p.as_str()) {
                    if !path.is_empty() && !path.starts_with('/') {
                        problems.push(format!(
                            "{at}.updateDataModel.path \"{path}\" is not a JSON Pointer — it must start with \"/\"."
                        ));
                    }
                }
            }
            "callRendererFunction" => {
                if !body
                    .get("functionCallId")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty())
                {
                    problems.push(format!(
                        "{at}.callRendererFunction needs a non-empty \"functionCallId\"."
                    ));
                }
                if !body
                    .get("callFunction")
                    .and_then(|f| f.get("call"))
                    .and_then(|c| c.as_str())
                    .is_some_and(|s| !s.is_empty())
                {
                    problems.push(format!(
                        "{at}.callRendererFunction.callFunction needs a \"call\" name."
                    ));
                }
            }
            "agentFunctionResponse" => {
                if !body
                    .get("functionCallId")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty())
                {
                    problems.push(format!(
                        "{at}.agentFunctionResponse needs a non-empty \"functionCallId\"."
                    ));
                }
                if !body.contains_key("value") && !body.contains_key("error") {
                    problems.push(format!(
                        "{at}.agentFunctionResponse needs either \"value\" or \"error\"."
                    ));
                }
            }
            _ => {}
        }
    }

    // A fresh surface has to be self-contained: it is the one moment where we
    // know every id the tree can reach. Incremental envelopes reference ids
    // declared by earlier calls, so their references are left alone.
    if has_create_surface {
        if !declared.is_empty() && !declared.iter().any(|id| id == "root") {
            problems.push(
                "No component has id \"root\". Exactly one component must, and it is the top of \
                 the tree."
                    .to_string(),
            );
        }
        for (from, id) in &referenced {
            if !declared.contains(id) {
                problems.push(format!(
                    "{from} points at \"{id}\", which no component in this envelope declares."
                ));
            }
        }
        // A Modal's trigger is the one Button that needs no action: opening
        // the Modal is its click.
        for (at, id) in &actionless {
            let is_trigger = referenced
                .iter()
                .any(|(from, target)| target == id && from.ends_with(".trigger"));
            if !is_trigger {
                problems.push(format!(
                    "{at} is a Button with no action, so a click on it goes nowhere. Give it \
                     \"action\": {{\"event\": {{\"name\": \"…\"}}}} (or openUrl / functionCall)."
                ));
            }
        }
    }

    problems
}

fn check_components(
    components: &Value,
    at: &str,
    problems: &mut Vec<String>,
    declared: &mut Vec<String>,
    referenced: &mut Vec<(String, String)>,
    actionless: &mut Vec<(String, String)>,
) {
    let Some(list) = components.as_array() else {
        problems.push(format!("{at} must be an array."));
        return;
    };
    if list.is_empty() {
        problems.push(format!("{at} is empty."));
        return;
    }
    for (i, component) in list.iter().enumerate() {
        let at = format!("{at}[{i}]");
        let Some(obj) = component.as_object() else {
            problems.push(format!("{at} is not a JSON object."));
            continue;
        };
        match obj.get("id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => declared.push(id.to_string()),
            _ => problems.push(format!("{at} needs a non-empty string \"id\".")),
        }
        let name = match obj.get("component").and_then(|v| v.as_str()) {
            Some(name) if !name.is_empty() => name,
            _ => {
                // `type` is the mistake models make most often here, and it is
                // worth naming rather than letting it read as a typo.
                let hint = if obj.contains_key("type") {
                    " (the key is \"component\", not \"type\")"
                } else {
                    ""
                };
                problems.push(format!("{at} needs a \"component\" name{hint}."));
                continue;
            }
        };
        if name == "Surface" {
            problems.push(format!(
                "{at} uses the reserved \"Surface\" component — the surface wraps your \"root\" \
                 component for you."
            ));
        } else if !is_known_component(name) {
            problems.push(format!(
                "{at} uses unknown component \"{name}\". The catalog is: {}.",
                CATALOG_COMPONENTS.join(", ")
            ));
        }
        if name == "Button" && !has_button_action(obj) {
            if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                actionless.push((at.clone(), id.to_string()));
            }
        }

        for prop in CHILD_PROPS {
            if let Some(id) = obj.get(*prop).and_then(|v| v.as_str()) {
                referenced.push((format!("{at}.{prop}"), id.to_string()));
            }
        }
        match obj.get("children") {
            // Static child list.
            Some(Value::Array(ids)) => {
                for (j, id) in ids.iter().enumerate() {
                    match id.as_str() {
                        Some(id) => referenced.push((format!("{at}.children[{j}]"), id.into())),
                        None => problems.push(format!(
                            "{at}.children[{j}] must be a component id string — children cannot \
                             be defined inline."
                        )),
                    }
                }
            }
            // ChildList template: {componentId, path}.
            Some(Value::Object(tpl)) => {
                match tpl.get("componentId").and_then(|v| v.as_str()) {
                    Some(id) => referenced.push((format!("{at}.children.componentId"), id.into())),
                    None => problems.push(format!(
                        "{at}.children is a template and needs a \"componentId\"."
                    )),
                }
                if !tpl.contains_key("path") {
                    problems.push(format!(
                        "{at}.children is a template and needs a \"path\" to the data list."
                    ));
                }
            }
            Some(_) => problems.push(format!(
                "{at}.children must be an array of ids or a {{componentId, path}} template."
            )),
            None => {}
        }
        if let Some(tabs) = obj.get("tabs").and_then(|t| t.as_array()) {
            for (j, tab) in tabs.iter().enumerate() {
                // `content` was the v0.9 spelling; both resolve to a child id.
                let child = tab
                    .get("child")
                    .or_else(|| tab.get("content"))
                    .and_then(|v| v.as_str());
                match child {
                    Some(id) => referenced.push((format!("{at}.tabs[{j}].child"), id.into())),
                    None => problems.push(format!("{at}.tabs[{j}] needs a \"child\" id.")),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One createSurface carrying a complete, valid tree.
    fn good() -> Vec<Value> {
        vec![json!({
            "version": "v1.0",
            "createSurface": {
                "surfaceId": "s1",
                "components": [
                    {"id": "root", "component": "Card", "child": "col"},
                    {"id": "col", "component": "Column", "children": ["title", "ok"]},
                    {"id": "title", "component": "Text", "text": "Hi"},
                    {"id": "label", "component": "Text", "text": "Go"},
                    {"id": "ok", "component": "Button", "child": "label",
                     "action": {"event": {"name": "go"}}}
                ],
                "dataModel": {"form": {}}
            }
        })]
    }

    #[test]
    fn a_complete_surface_has_no_problems() {
        assert_eq!(validate(&good()), Vec::<String>::new());
    }

    #[test]
    fn fill_surface_ids_inherits_from_the_single_create_surface() {
        let mut messages = vec![
            json!({"createSurface": {"surfaceId": "s1"}}),
            json!({"updateComponents": {"components": []}}),
            json!({"updateDataModel": {"surfaceId": "", "path": "/a", "value": 1}}),
            json!({"updateDataModel": {"surfaceId": "other", "path": "/b", "value": 2}}),
        ];
        fill_surface_ids(&mut messages);
        assert_eq!(messages[1]["updateComponents"]["surfaceId"], "s1");
        assert_eq!(messages[2]["updateDataModel"]["surfaceId"], "s1");
        assert_eq!(messages[3]["updateDataModel"]["surfaceId"], "other");

        // Two surfaces created — ambiguous, leave the envelope alone.
        let mut two = vec![
            json!({"createSurface": {"surfaceId": "a"}}),
            json!({"createSurface": {"surfaceId": "b"}}),
            json!({"updateComponents": {"components": []}}),
        ];
        fill_surface_ids(&mut two);
        assert!(two[2]["updateComponents"].get("surfaceId").is_none());
    }

    #[test]
    fn normalize_stamps_version_and_catalog() {
        let mut messages = vec![json!({"createSurface": {"surfaceId": "s1"}})];
        normalize(&mut messages);
        assert_eq!(messages[0]["version"], PROTOCOL_VERSION);
        assert_eq!(messages[0]["createSurface"]["catalogId"], BASIC_CATALOG_ID);
    }

    #[test]
    fn normalize_keeps_an_explicit_catalog() {
        let mut messages =
            vec![json!({"createSurface": {"surfaceId": "s1", "catalogId": "acme.com:custom"}})];
        normalize(&mut messages);
        assert_eq!(messages[0]["createSurface"]["catalogId"], "acme.com:custom");
    }

    #[test]
    fn a_wrong_protocol_version_is_reported() {
        let messages = vec![json!({"version": "v0.9", "deleteSurface": {"surfaceId": "s1"}})];
        let problems = validate(&messages);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("v0.9"), "{problems:?}");
    }

    #[test]
    fn every_v1_message_kind_is_accepted() {
        let messages = vec![
            json!({"version": "v1.0", "createSurface": {"surfaceId": "s1"}}),
            json!({"version": "v1.0", "updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Text", "text": "hi"}
            ]}}),
            json!({"version": "v1.0", "updateDataModel": {"surfaceId": "s1", "path": "/a", "value": 1}}),
            json!({"version": "v1.0", "callRendererFunction": {
                "functionCallId": "c1",
                "callFunction": {"call": "openUrl", "catalogId": BASIC_CATALOG_ID,
                                 "args": {"url": "https://example.com"}}
            }}),
            json!({"version": "v1.0", "agentFunctionResponse": {"functionCallId": "c1", "value": 7}}),
            json!({"version": "v1.0", "deleteSurface": {"surfaceId": "s1"}}),
        ];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn a_message_must_carry_exactly_one_kind() {
        let none = validate(&[json!({"version": "v1.0"})]);
        assert!(none[0].contains("no message kind"), "{none:?}");

        let both = validate(&[json!({
            "createSurface": {"surfaceId": "s1"},
            "deleteSurface": {"surfaceId": "s1"}
        })]);
        assert!(both[0].contains("2 message kinds"), "{both:?}");
    }

    #[test]
    fn a_surface_id_is_required() {
        let problems = validate(&[json!({"updateDataModel": {"value": 1}})]);
        assert!(
            problems.iter().any(|p| p.contains("surfaceId")),
            "{problems:?}"
        );
    }

    #[test]
    fn update_data_model_needs_a_value_and_a_pointer_path() {
        let missing = validate(&[json!({"updateDataModel": {"surfaceId": "s1"}})]);
        assert!(
            missing.iter().any(|p| p.contains("\"value\"")),
            "{missing:?}"
        );

        let bad_path = validate(&[
            json!({"updateDataModel": {"surfaceId": "s1", "path": "form/x", "value": 1}}),
        ]);
        assert!(
            bad_path.iter().any(|p| p.contains("JSON Pointer")),
            "{bad_path:?}"
        );
    }

    #[test]
    fn a_null_value_is_a_delete_not_a_missing_value() {
        let messages =
            vec![json!({"updateDataModel": {"surfaceId": "s1", "path": "/a", "value": null}})];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn an_unknown_component_names_the_catalog() {
        let messages = vec![
            json!({"updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Accordion"}
            ]}}),
        ];
        let problems = validate(&messages);
        assert!(problems[0].contains("Accordion"), "{problems:?}");
        assert!(problems[0].contains("TextField"), "{problems:?}");
    }

    #[test]
    fn type_instead_of_component_gets_a_pointed_hint() {
        let messages = vec![
            json!({"updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "type": "Card"}
            ]}}),
        ];
        let problems = validate(&messages);
        assert!(problems[0].contains("not \"type\""), "{problems:?}");
    }

    #[test]
    fn the_reserved_surface_component_is_rejected() {
        let messages = vec![
            json!({"updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Surface", "child": "x"}
            ]}}),
        ];
        let problems = validate(&messages);
        assert!(problems[0].contains("reserved"), "{problems:?}");
    }

    #[test]
    fn legacy_render_ui_components_still_validate() {
        let messages = vec![
            json!({"updateComponents": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "Heading", "text": "Hi", "level": 2}
            ]}}),
        ];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn a_new_surface_must_declare_root() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "body", "component": "Text", "text": "hi"}
        ]}})];
        let problems = validate(&messages);
        assert!(
            problems.iter().any(|p| p.contains("\"root\"")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_create_surface_without_components_may_wait_for_them() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1"}})];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn dangling_child_references_are_caught_on_a_new_surface() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "root", "component": "Card", "child": "gone"}
        ]}})];
        let problems = validate(&messages);
        assert!(
            problems.iter().any(|p| p.contains("\"gone\"")),
            "{problems:?}"
        );
    }

    #[test]
    fn incremental_updates_may_reference_earlier_components() {
        // No createSurface: the ids live on a surface a previous call built.
        let messages = vec![
            json!({"updateComponents": {"surfaceId": "s1", "components": [
                {"id": "row", "component": "Row", "children": ["built", "earlier"]}
            ]}}),
        ];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn inline_children_are_rejected() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "root", "component": "Column",
             "children": [{"id": "nested", "component": "Text", "text": "no"}]}
        ]}})];
        let problems = validate(&messages);
        assert!(
            problems.iter().any(|p| p.contains("inline")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_child_list_template_needs_a_component_id_and_path() {
        let ok = validate(
            &[json!({"createSurface": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "List", "children": {"componentId": "row", "path": "/items"}},
                {"id": "row", "component": "Text", "text": {"path": "name"}}
            ]}})],
        );
        assert_eq!(ok, Vec::<String>::new());

        let bad = validate(
            &[json!({"createSurface": {"surfaceId": "s1", "components": [
                {"id": "root", "component": "List", "children": {"componentId": "row"}},
                {"id": "row", "component": "Text", "text": "x"}
            ]}})],
        );
        assert!(bad.iter().any(|p| p.contains("\"path\"")), "{bad:?}");
    }

    #[test]
    fn tabs_children_are_resolved_in_either_spelling() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "root", "component": "Tabs", "tabs": [
                {"title": "One", "child": "p1"},
                {"title": "Two", "content": "p2"}
            ]},
            {"id": "p1", "component": "Text", "text": "1"},
            {"id": "p2", "component": "Text", "text": "2"}
        ]}})];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn a_tab_without_a_child_is_reported() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "root", "component": "Tabs", "tabs": [{"title": "One"}]}
        ]}})];
        let problems = validate(&messages);
        assert!(
            problems.iter().any(|p| p.contains("tabs[0]")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_function_call_needs_an_id_and_a_name() {
        let problems = validate(&[json!({"callRendererFunction": {"callFunction": {}}})]);
        assert!(
            problems.iter().any(|p| p.contains("functionCallId")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("\"call\"")),
            "{problems:?}"
        );
    }

    /// The gallery envelopes shipped with the v1.0 basic catalog, verbatim
    /// (a2ui-project/a2ui, `specification/v1_0/catalogs/basic/examples`,
    /// Apache-2.0). If the real thing trips our validator, the validator is
    /// wrong.
    #[test]
    fn the_official_v1_gallery_examples_all_validate() {
        const EXAMPLES: &[(&str, &str)] = &[
            (
                "09_login-form",
                include_str!("../tests/fixtures/a2ui/09_login-form.json"),
            ),
            (
                "27_stats-card",
                include_str!("../tests/fixtures/a2ui/27_stats-card.json"),
            ),
            (
                "31_incremental-dashboard",
                include_str!("../tests/fixtures/a2ui/31_incremental-dashboard.json"),
            ),
            (
                "32_advanced-form-validator",
                include_str!("../tests/fixtures/a2ui/32_advanced-form-validator.json"),
            ),
            (
                "33_financial-data-grid",
                include_str!("../tests/fixtures/a2ui/33_financial-data-grid.json"),
            ),
            (
                "34_child-list-template",
                include_str!("../tests/fixtures/a2ui/34_child-list-template.json"),
            ),
            (
                "35_markdown-text",
                include_str!("../tests/fixtures/a2ui/35_markdown-text.json"),
            ),
            (
                "36_modal",
                include_str!("../tests/fixtures/a2ui/36_modal.json"),
            ),
        ];
        for (name, raw) in EXAMPLES {
            let doc: Value = serde_json::from_str(raw).expect(name);
            let messages: Vec<Value> = doc["messages"].as_array().expect(name).clone();
            assert_eq!(validate(&messages), Vec::<String>::new(), "{name}");
        }
    }

    #[test]
    fn every_problem_in_an_envelope_is_reported_at_once() {
        let messages = vec![json!({"createSurface": {"surfaceId": "", "components": [
            {"component": "Nope"},
            {"id": "root", "component": "Card", "child": "missing"}
        ]}})];
        let problems = validate(&messages);
        assert!(problems.len() >= 4, "{problems:?}");
    }

    #[test]
    fn a_button_without_an_action_is_rejected_on_a_new_surface() {
        // What an agent actually sent: the only Button has no action, so the
        // renderer drew a broken control and the blocked call could never end.
        let messages = vec![
            json!({"createSurface": {"surfaceId": "pick", "components": [
                {"id": "root", "component": "Card", "child": "go"},
                {"id": "go", "component": "Button", "child": "label", "variant": "primary"},
                {"id": "label", "component": "Text", "text": "Open it"}
            ]}}),
        ];
        let problems = validate(&messages);
        assert!(
            problems.iter().any(|p| p.contains("Button with no action")),
            "{problems:?}"
        );
    }

    #[test]
    fn every_button_action_the_renderer_accepts_validates() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "root", "component": "Row", "children": ["a", "b", "c", "d"]},
            {"id": "a", "component": "Button", "child": "t", "action": {"event": {"name": "ok"}}},
            {"id": "b", "component": "Button", "child": "t",
             "action": {"functionCall": {"call": "openUrl", "args": {"url": "https://x.test"}}}},
            {"id": "c", "component": "Button", "child": "t", "action": {"openUrl": {"url": "https://x.test"}}},
            {"id": "d", "component": "Button", "child": "t", "actions": {"event": {"name": "no"}}},
            {"id": "t", "component": "Text", "text": "Go"}
        ]}})];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn a_modal_trigger_needs_no_action() {
        let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "root", "component": "Modal", "trigger": "open", "content": "body"},
            {"id": "open", "component": "Button", "child": "label"},
            {"id": "label", "component": "Text", "text": "Details"},
            {"id": "body", "component": "Text", "text": "More"}
        ]}})];
        assert_eq!(validate(&messages), Vec::<String>::new());
    }

    #[test]
    fn a_blocking_surface_needs_a_button_that_sends_an_event() {
        let awaited = vec!["pick".to_string()];
        let no_event = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
            {"id": "root", "component": "ChoicePicker", "options": []}
        ]}})];
        let reason = unanswerable_wait(&no_event, &awaited);
        assert!(reason.is_some_and(|r| r.contains("pick")));

        assert_eq!(unanswerable_wait(&good(), &awaited), None);
        assert_eq!(unanswerable_wait(&no_event, &[]), None);
        // Updating an existing surface: its Buttons may come from an earlier call.
        let update =
            vec![json!({"updateDataModel": {"surfaceId": "s1", "path": "/a", "value": 1}})];
        assert_eq!(unanswerable_wait(&update, &awaited), None);
    }
}
