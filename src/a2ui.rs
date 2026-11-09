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

use serde_json::{json, Value};

/// Stamped on every message the agent sends us.
pub const PROTOCOL_VERSION: &str = "1.0";

/// Surface default when the agent doesn't name a catalog.
pub const BASIC_CATALOG_ID: &str = "https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json";

/// One basic-catalog component, copied from `catalogs/basic/v1/catalog.json`:
/// the props it must carry, the ones it may, and the closed value sets of its
/// string props. `checks` is allowed only where the catalog applies `Checkable`.
struct ComponentSpec {
	name: &'static str,
	required: &'static [&'static str],
	optional: &'static [&'static str],
	enums: &'static [(&'static str, &'static [&'static str])],
	checkable: bool,
}

/// Props every catalog component accepts on top of its own.
const COMMON_PROPS: &[&str] = &["id", "component", "weight", "accessibility", "catalogId", "metadata"];

/// The v1.0 basic catalog's functions, plus the universal `@index`.
const CATALOG_FUNCTIONS: &[&str] = &[
	"required",
	"regex",
	"length",
	"numeric",
	"email",
	"formatString",
	"formatNumber",
	"formatCurrency",
	"formatDate",
	"pluralize",
	"openUrl",
	"and",
	"or",
	"not",
	"@index",
];

/// The basic catalog's Icon `name` values; any other literal draws as plain text.
const ICON_NAMES: &[&str] = &[
	"accountCircle",
	"add",
	"arrowBack",
	"arrowForward",
	"attachFile",
	"calendarToday",
	"call",
	"camera",
	"check",
	"close",
	"delete",
	"download",
	"edit",
	"event",
	"error",
	"fastForward",
	"favorite",
	"favoriteOff",
	"folder",
	"help",
	"home",
	"info",
	"locationOn",
	"lock",
	"lockOpen",
	"mail",
	"menu",
	"moreVert",
	"moreHoriz",
	"notificationsOff",
	"notifications",
	"pause",
	"payment",
	"person",
	"phone",
	"photo",
	"play",
	"print",
	"refresh",
	"rewind",
	"search",
	"send",
	"settings",
	"share",
	"shoppingCart",
	"skipNext",
	"skipPrevious",
	"star",
	"starHalf",
	"starOff",
	"stop",
	"upload",
	"visibility",
	"visibilityOff",
	"volumeDown",
	"volumeMute",
	"volumeOff",
	"volumeUp",
	"warning",
];

/// The v1.0 basic catalog, in catalog order.
const COMPONENT_SPECS: &[ComponentSpec] = &[
	ComponentSpec {
		name: "Text",
		required: &["text"],
		optional: &["variant"],
		enums: &[("variant", &["caption", "body"])],
		checkable: false,
	},
	ComponentSpec {
		name: "Image",
		required: &["url"],
		optional: &["description", "fit", "variant"],
		enums: &[
			("fit", &["contain", "cover", "fill", "none", "scaleDown"]),
			(
				"variant",
				&[
					"icon",
					"avatar",
					"smallFeature",
					"mediumFeature",
					"largeFeature",
					"header",
				],
			),
		],
		checkable: false,
	},
	ComponentSpec {
		name: "Icon",
		required: &["name"],
		optional: &[],
		enums: &[("name", ICON_NAMES)],
		checkable: false,
	},
	ComponentSpec {
		name: "Video",
		required: &["url"],
		optional: &["posterUrl"],
		enums: &[],
		checkable: false,
	},
	ComponentSpec {
		name: "AudioPlayer",
		required: &["url"],
		optional: &["description"],
		enums: &[],
		checkable: false,
	},
	ComponentSpec {
		name: "Row",
		required: &["children"],
		optional: &["justify", "align"],
		enums: &[
			(
				"justify",
				&[
					"center",
					"end",
					"spaceAround",
					"spaceBetween",
					"spaceEvenly",
					"start",
					"stretch",
				],
			),
			("align", &["start", "center", "end", "stretch"]),
		],
		checkable: false,
	},
	ComponentSpec {
		name: "Column",
		required: &["children"],
		optional: &["justify", "align"],
		enums: &[
			(
				"justify",
				&[
					"start",
					"center",
					"end",
					"spaceBetween",
					"spaceAround",
					"spaceEvenly",
					"stretch",
				],
			),
			("align", &["center", "end", "start", "stretch"]),
		],
		checkable: false,
	},
	ComponentSpec {
		name: "List",
		required: &["children"],
		optional: &["direction", "align"],
		enums: &[
			("direction", &["vertical", "horizontal"]),
			("align", &["start", "center", "end", "stretch"]),
		],
		checkable: false,
	},
	ComponentSpec {
		name: "Card",
		required: &["child"],
		optional: &[],
		enums: &[],
		checkable: false,
	},
	ComponentSpec {
		name: "Tabs",
		required: &["tabs"],
		optional: &[],
		enums: &[],
		checkable: false,
	},
	ComponentSpec {
		name: "Modal",
		required: &["trigger", "content"],
		optional: &[],
		enums: &[],
		checkable: false,
	},
	ComponentSpec {
		name: "Divider",
		required: &[],
		optional: &["axis"],
		enums: &[("axis", &["horizontal", "vertical"])],
		checkable: false,
	},
	ComponentSpec {
		name: "Button",
		required: &["child", "action"],
		optional: &["variant"],
		enums: &[("variant", &["default", "primary", "borderless"])],
		checkable: true,
	},
	ComponentSpec {
		name: "TextField",
		required: &["label", "value"],
		optional: &["placeholder", "variant"],
		enums: &[("variant", &["longText", "number", "shortText", "obscured"])],
		checkable: true,
	},
	ComponentSpec {
		name: "CheckBox",
		required: &["label", "value"],
		optional: &[],
		enums: &[],
		checkable: true,
	},
	ComponentSpec {
		name: "ChoicePicker",
		required: &["options", "value"],
		optional: &["label", "variant", "displayStyle", "filterable"],
		enums: &[
			("variant", &["multipleSelection", "mutuallyExclusive"]),
			("displayStyle", &["checkbox", "chips"]),
		],
		checkable: true,
	},
	ComponentSpec {
		name: "Slider",
		required: &["value", "max"],
		optional: &["label", "min", "steps"],
		enums: &[],
		checkable: true,
	},
	ComponentSpec {
		name: "DateTimeInput",
		required: &["value"],
		optional: &["enableDate", "enableTime", "min", "max", "label"],
		enums: &[],
		checkable: true,
	},
];

/// The message kinds that name a surface.
const SURFACE_KINDS: &[&str] = &["createSurface", "updateComponents", "updateDataModel", "deleteSurface"];

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

/// A Button needs `action.event.name` or `action.functionCall.@call` to send a
/// click anywhere; without one the renderer draws a broken control.
fn has_button_action(button: &serde_json::Map<String, Value>) -> bool {
	let Some(action) = button.get("action") else {
		return false;
	};
	action
		.pointer("/event/name")
		.and_then(Value::as_str)
		.is_some_and(|name| !name.is_empty())
		|| action.pointer("/functionCall/@call").is_some_and(Value::is_string)
}

/// Report a component's props against its catalog entry: missing required
/// props, props the component does not have (naming the ones it does), and
/// string values outside a closed set.
fn check_component_props(
	obj: &serde_json::Map<String, Value>,
	spec: &ComponentSpec,
	at: &str,
	problems: &mut Vec<String>,
) {
	for prop in spec.required {
		if !obj.contains_key(*prop) {
			problems.push(format!("{at} ({}) is missing required \"{prop}\".", spec.name));
		}
	}
	let allowed = || {
		COMMON_PROPS
			.iter()
			.chain(spec.required)
			.chain(spec.optional)
			.chain(spec.checkable.then_some(&"checks"))
	};
	for key in obj.keys().filter(|key| !allowed().any(|prop| *prop == key.as_str())) {
		problems.push(format!(
			"{at}.{key} is not a {} prop. {} accepts: {}.",
			spec.name,
			spec.name,
			allowed().copied().collect::<Vec<_>>().join(", ")
		));
	}
	for (prop, values) in spec.enums {
		if let Some(value) = obj.get(*prop).and_then(Value::as_str).filter(|v| !values.contains(v)) {
			problems.push(format!("{at}.{prop} \"{value}\" is not one of: {}.", values.join(", ")));
		}
	}
}

/// Why a blocking `render_ui` could never be answered: it waits on
/// `await_events`, but nothing on the surface sends an event. Any event click
/// completes the call, so only a surface with none is stuck until the timeout.
/// A Button that only opens a Modal (it is the Modal's `trigger`) sends
/// nothing. Incremental envelopes may be answered by Buttons an earlier call
/// drew, unless they delete the surface and draw no new one.
pub fn unanswerable_wait(messages: &[Value], await_events: &[String]) -> Option<String> {
	let creates_surface = messages.iter().any(|m| m.get("createSurface").is_some());
	let deletes_surface = messages.iter().any(|m| m.get("deleteSurface").is_some());
	if await_events.is_empty() {
		return None;
	}
	if deletes_surface && !creates_surface {
		return Some(format!(
			"await_events is {await_events:?}, but this envelope deletes the surface and draws no \
             new one, so there is nothing to click and the call would wait until it times out. \
             Drop await_events."
		));
	}
	if !creates_surface {
		return None;
	}
	let components: Vec<&serde_json::Map<String, Value>> = messages
		.iter()
		.filter_map(|m| m.get("createSurface").or_else(|| m.get("updateComponents")))
		.filter_map(|body| body.get("components")?.as_array())
		.flatten()
		.filter_map(Value::as_object)
		.collect();
	let modal_triggers: Vec<&str> = components
		.iter()
		.filter(|c| c.get("component").and_then(Value::as_str) == Some("Modal"))
		.filter_map(|modal| modal.get("trigger")?.as_str())
		.collect();
	let sends_event = components
		.iter()
		.filter(|c| c.get("component").and_then(Value::as_str) == Some("Button"))
		.filter(|c| {
			!c.get("id")
				.and_then(Value::as_str)
				.is_some_and(|id| modal_triggers.contains(&id))
		})
		.filter_map(|button| button.get("action"))
		.any(|action| {
			action
				.pointer("/event/name")
				.and_then(Value::as_str)
				.is_some_and(|n| !n.is_empty())
		});
	(!sends_event).then(|| {
		format!(
			"await_events is {await_events:?}, but no Button on this surface has an \
             \"action\": {{\"event\": {{\"name\"}}}} (a Button that only opens a Modal does not \
             count), so nothing can answer and the call would wait until it times out. Give a \
             Button one of those event names, or drop await_events for a fire-and-forget update."
		)
	})
}

/// The message kind of `msg`, ignoring the `version` envelope field.
fn kinds_of(msg: &serde_json::Map<String, Value>) -> Vec<&'static str> {
	MESSAGE_KINDS.iter().copied().filter(|k| msg.contains_key(*k)).collect()
}

/// Bring an envelope up to v1.0 shape: stamp the protocol version on every
/// message, give a surface the basic catalog when it names none, and turn
/// `sendDataModel` on when it is absent. The first two are required by the
/// schema but neither changes what gets drawn, so we fill them in rather than
/// rejecting an envelope over them. The spec defaults `sendDataModel` to false;
/// octoweb stamps true at this agent layer so a form's values reach the agent
/// unless it opts out with an explicit false.
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
			surface.entry("sendDataModel").or_insert(Value::Bool(true));
		}
	}
}

/// Everything wrong with this envelope, in the order it was found. Empty means
/// the renderer can draw it.
pub fn validate(messages: &[Value]) -> Vec<String> {
	let mut problems = Vec::new();
	// Component ids declared anywhere in this envelope, as (path, id), and the
	// references they make — checked together at the end, once every message
	// is seen.
	let mut declared: Vec<(String, String)> = Vec::new();
	let mut referenced: Vec<(String, String)> = Vec::new();
	// Buttons with nowhere for a click to go, as (path, id).
	let mut actionless: Vec<(String, String)> = Vec::new();
	let mut has_create_surface = false;
	// Distinct surfaces the envelope touches, in the order first seen.
	let mut surfaces: Vec<&str> = Vec::new();

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

		if SURFACE_KINDS.contains(&kind) {
			match body.get("surfaceId").and_then(|s| s.as_str()).filter(|s| !s.is_empty()) {
				Some(id) if !surfaces.contains(&id) => surfaces.push(id),
				Some(_) => {}
				None => problems.push(format!("{at}.{kind} needs a non-empty \"surfaceId\".")),
			}
		}

		match kind {
			"createSurface" => {
				has_create_surface = true;
				check_catalog_id(body, &format!("{at}.createSurface"), &mut problems);
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
				None => problems.push(format!("{at}.updateComponents needs a \"components\" array.")),
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
					.and_then(|f| f.get("@call"))
					.and_then(|c| c.as_str())
					.is_some_and(|s| !s.is_empty())
				{
					problems.push(format!(
						"{at}.callRendererFunction.callFunction needs a \"@call\" name."
					));
				}
				// The spec makes the catalog explicit on a call; there is no default to fall back on.
				let call = body.get("callFunction").and_then(Value::as_object);
				if call.is_some_and(|f| !f.contains_key("catalogId")) {
					problems.push(format!(
						"{at}.callRendererFunction.callFunction needs a \"catalogId\" (use {BASIC_CATALOG_ID})."
					));
				}
				if let Some(call) = call {
					check_catalog_id(call, &format!("{at}.callRendererFunction.callFunction"), &mut problems);
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
		if !declared.is_empty() && !declared.iter().any(|(_, id)| id == "root") {
			problems.push(
				"No component has id \"root\". Exactly one component must, and it is the top of \
                 the tree."
					.to_string(),
			);
		}
		for (from, id) in &referenced {
			if !declared.iter().any(|(_, d)| d == id) {
				problems.push(format!(
					"{from} points at \"{id}\", which no component in this envelope declares."
				));
			}
		}
		// The surface is a tree: a component listed by two parents is drawn
		// twice, e.g. a Button's label Text also in the Row beside the Button.
		// A template is stamped out per row, so two Lists may share one. A later
		// definition of an id replaces the earlier one, so what that one listed
		// is no longer a parent.
		let is_parent = |from: &str| {
			let replaced = declared.iter().enumerate().any(|(i, (at, id))| {
				from.starts_with(&format!("{at}.")) && declared[i + 1..].iter().any(|(_, later)| later == id)
			});
			!replaced && !from.ends_with(".children.componentId")
		};
		for (k, (from, id)) in referenced.iter().enumerate().filter(|(_, (from, _))| is_parent(from)) {
			let first = referenced
				.iter()
				.position(|(from, target)| target == id && is_parent(from));
			if let Some(first) = first.filter(|&first| first < k) {
				problems.push(format!(
					"{from} lists \"{id}\", which {} already lists, so it would render twice. \
                     Each component has exactly one parent: give the second use its own id.",
					referenced[first].0
				));
			}
		}
		// The renderer walks the tree down from "root", so a component no
		// parent lists is silently never drawn — and a Button whose checks
		// read its value can then never be clicked through.
		for (at, id) in &declared {
			if id != "root" && !referenced.iter().any(|(_, target)| target == id) {
				problems.push(format!(
					"{at} declares \"{id}\", but no component lists it as a child, so it never \
                     renders. Add \"{id}\" to a parent's \"children\" (or \"child\"), or drop it."
				));
			}
		}
		for (at, _) in &actionless {
			problems.push(format!(
				"{at}.action has no event name or function call, so a click on the Button goes nowhere. Give it \
                 {{\"event\": {{\"name\": \"…\"}}}} or {{\"functionCall\": {{\"@call\": \"openUrl\", \"args\": {{\"url\": \"…\"}}}}}}."
			));
		}
	}

	if surfaces.len() > 1 {
		problems.push(format!(
			"This envelope touches {} surfaces ({}). Send one surface per render_ui call: one \
             chat bubble and one blocked call map to one surface.",
			surfaces.len(),
			surfaces.join(", ")
		));
	}

	problems
}

/// The basic catalog is the only one octoweb has, so any other `catalogId` names
/// components or functions it cannot draw or run.
fn check_catalog_id(owner: &serde_json::Map<String, Value>, at: &str, problems: &mut Vec<String>) {
	if let Some(id) = owner
		.get("catalogId")
		.filter(|id| id.as_str() != Some(BASIC_CATALOG_ID))
	{
		problems.push(format!(
			"{at}.catalogId is {id}, but the only catalog this renderer has is \"{BASIC_CATALOG_ID}\"."
		));
	}
}

/// The surface a message addresses, with the kind that names it.
fn surface_message(msg: &Value) -> Option<(&'static str, &str)> {
	SURFACE_KINDS.iter().find_map(|kind| {
		let id = msg.get(*kind)?.get("surfaceId")?.as_str().filter(|id| !id.is_empty())?;
		Some((*kind, id))
	})
}

/// A renderer → agent `error` message (spec `error`).
fn error_message(code: &str, surface_id: &str, path: &str, message: &str) -> Value {
	json!({
		"version": PROTOCOL_VERSION,
		"error": {"code": code, "surfaceId": surface_id, "path": path, "message": message},
	})
}

/// JSON Pointer to the leading `messages[i].kind[j]…` accessor chain of a
/// `validate` problem, or "/" for a problem that is about the envelope as a whole.
fn problem_pointer(problem: &str) -> String {
	let Some(mut rest) = problem.strip_prefix("messages") else {
		return "/".to_string();
	};
	let mut pointer = String::from("/messages");
	loop {
		if let Some(after) = rest.strip_prefix('[') {
			let Some((index, tail)) = after.split_once(']') else {
				break;
			};
			if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
				break;
			}
			pointer.push('/');
			pointer.push_str(index);
			rest = tail;
		} else if let Some(after) = rest.strip_prefix('.') {
			let end = after
				.find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '@' | '-')))
				.unwrap_or(after.len());
			if end == 0 {
				break;
			}
			pointer.push('/');
			pointer.push_str(&after[..end].replace('~', "~0").replace('/', "~1"));
			rest = &after[end..];
		} else {
			break;
		}
	}
	pointer
}

/// The tool error for an envelope `validate` (or `unanswerable_wait`) turned
/// down: the sentence the agents already know, then one spec `error` message
/// per problem, one JSON object per line.
pub fn rejection_text(problems: &[String], messages: &[Value]) -> String {
	let surface_id = messages.iter().find_map(surface_message).map_or("", |(_, id)| id);
	let lines: Vec<String> = problems
		.iter()
		.map(|problem| error_message("VALIDATION_FAILED", surface_id, &problem_pointer(problem), problem).to_string())
		.collect();
	format!("This A2UI envelope will not render:\n{}", lines.join("\n"))
}

/// The basic catalog declares no `allowedCallers`, which makes every function
/// `rendererOnly`: an agent may not call any of them. Take every
/// `callRendererFunction` out of the envelope and answer each with the
/// `rendererFunctionResponse` the spec prescribes, without the renderer ever
/// seeing the call.
pub fn reject_renderer_calls(messages: &mut Vec<Value>) -> Vec<Value> {
	let mut responses = Vec::new();
	messages.retain(|msg| {
		let Some(call) = msg.get("callRendererFunction") else {
			return true;
		};
		let id = call["functionCallId"].as_str().unwrap_or_default();
		let name = call["callFunction"]["@call"].as_str().unwrap_or_default();
		responses.push(json!({
			"version": PROTOCOL_VERSION,
			"rendererFunctionResponse": {
				"functionCallId": id,
				"error": {
					"code": "INVALID_FUNCTION_CALL",
					"message": format!(
						"\"{name}\" is rendererOnly in the basic catalog, so an agent cannot call it. \
						 Put it in a component property or a Button action."
					),
				},
			},
		}));
		false
	});
	responses
}

/// Surfaces that exist after `msg`, per the lifecycle rules: `createSurface`
/// adds one and `deleteSurface` removes it.
fn track_surface(live: &mut std::collections::HashSet<String>, msg: &Value) {
	match surface_message(msg) {
		Some(("createSurface", id)) => {
			live.insert(id.to_string());
		}
		Some(("deleteSurface", id)) => {
			live.remove(id);
		}
		_ => {}
	}
}

/// Check `incoming` against the surfaces this session already has. `saved` is
/// the session's stored A2UI bodies, oldest first; only their v1.0 messages
/// count. Creating a live surface is an `INTEGRITY_ERROR`; updating or deleting
/// one that is not live is `VALIDATION_FAILED`. Returns the spec `error`
/// message as JSON text for the first violation.
pub fn surface_lifecycle_error<'a>(saved: impl IntoIterator<Item = &'a Value>, incoming: &[Value]) -> Option<String> {
	let mut live = std::collections::HashSet::new();
	for body in saved {
		let messages = body.get("messages").and_then(Value::as_array).into_iter().flatten();
		for msg in messages.filter(|m| m.get("version").and_then(Value::as_str) == Some(PROTOCOL_VERSION)) {
			track_surface(&mut live, msg);
		}
	}
	for (i, msg) in incoming.iter().enumerate() {
		let Some((kind, id)) = surface_message(msg) else {
			continue;
		};
		let violation = match (kind, live.contains(id)) {
			("createSurface", true) => Some((
				"INTEGRITY_ERROR",
				format!(
					"Surface \"{id}\" already exists. Send updateComponents or updateDataModel to change it, \
					 or deleteSurface first to recreate it."
				),
			)),
			("createSurface", false) | (_, true) => None,
			(_, false) => Some((
				"VALIDATION_FAILED",
				format!("Surface \"{id}\" does not exist. Send createSurface for it first."),
			)),
		};
		if let Some((code, message)) = violation {
			return Some(error_message(code, id, &format!("/messages/{i}/{kind}"), &message).to_string());
		}
		track_surface(&mut live, msg);
	}
	None
}

/// A click as it goes to the agent: the spec `action` message, plus the
/// surface's data model under `metadata` when the surface sends it. The
/// internal resolution (with `actor`) stays what the sidebar and saved state use.
pub fn action_message(resolution: &Value) -> Value {
	let mut action = serde_json::Map::new();
	for key in [
		"name",
		"surfaceId",
		"sourceComponentId",
		"timestamp",
		"context",
		"userMessage",
	] {
		if let Some(value) = resolution.get(key).filter(|v| !v.is_null()) {
			action.insert(key.to_string(), value.clone());
		}
	}
	let mut message = json!({"version": PROTOCOL_VERSION, "action": action});
	if let Some(model) = resolution.get("dataModel").filter(|m| !m.is_null()) {
		let surface_id = resolution["surfaceId"].as_str().unwrap_or_default();
		message["metadata"] = json!({
			"a2uiRendererDataModel": {
				"version": PROTOCOL_VERSION,
				"surfaces": {surface_id: model},
			},
		});
	}
	message
}

fn check_components(
	components: &Value,
	at: &str,
	problems: &mut Vec<String>,
	declared: &mut Vec<(String, String)>,
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
			Some(id) if !id.is_empty() => declared.push((at.clone(), id.to_string())),
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
		check_catalog_id(obj, &at, problems);
		if name == "Surface" {
			problems.push(format!(
				"{at} uses the reserved \"Surface\" component — the surface wraps your \"root\" \
                 component for you."
			));
		} else if let Some(spec) = COMPONENT_SPECS.iter().find(|spec| spec.name == name) {
			check_component_props(obj, spec, &at, problems);
		} else {
			problems.push(format!(
				"{at} uses unknown component \"{name}\". The catalog is: {}.",
				COMPONENT_SPECS
					.iter()
					.map(|spec| spec.name)
					.collect::<Vec<_>>()
					.join(", ")
			));
		}
		if name == "Button" && obj.contains_key("action") && !has_button_action(obj) {
			if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
				actionless.push((at.clone(), id.to_string()));
			}
		}

		for prop in CHILD_PROPS {
			if let Some(id) = obj.get(*prop).and_then(|v| v.as_str()) {
				referenced.push((format!("{at}.{prop}"), id.to_string()));
			}
		}
		for (prop, value) in obj.iter().filter(|(prop, _)| *prop != "children") {
			check_dynamic_values(value, &format!("{at}.{prop}"), problems);
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
					None => problems.push(format!("{at}.children is a template and needs a \"componentId\".")),
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
				let child = tab.get("child").and_then(|v| v.as_str());
				match child {
					Some(id) => referenced.push((format!("{at}.tabs[{j}].child"), id.into())),
					None => problems.push(format!("{at}.tabs[{j}] needs a \"child\" id.")),
				}
			}
		}
	}
}

/// Walks a prop value for the two ways a v1.0 DynamicValue goes wrong: an unescaped
/// reserved `@` key, and a binding or call written without its `@` (which would render
/// silently as literal data). `children` is skipped by the caller: its `path` is plain.
fn check_dynamic_values(value: &Value, at: &str, problems: &mut Vec<String>) {
	match value {
		Value::Object(map) => {
			if map.contains_key("@call") {
				check_catalog_id(map, at, problems);
				if let Some(name) = map
					.get("@call")
					.and_then(Value::as_str)
					.filter(|name| !CATALOG_FUNCTIONS.contains(name))
				{
					problems.push(format!(
						"{at}.@call \"{name}\" is not a catalog function. The catalog has: {}.",
						CATALOG_FUNCTIONS.join(", ")
					));
				}
			}
			for key in map
				.keys()
				.filter(|k| is_reserved_key(k) && *k != "@path" && *k != "@call")
			{
				problems.push(format!(
					"{at} uses reserved key \"{key}\"; escape a literal leading @ as @@."
				));
			}
			let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
			keys.sort_unstable();
			match keys.as_slice() {
				["path"] => problems.push(format!(
					"{at} is {{\"path\": ...}}, which is literal data. To bind, write {{\"@path\": ...}}."
				)),
				["call"] | ["args", "call"] => problems.push(format!(
					"{at} is {{\"call\": ...}}, which is literal data. To call a function, write {{\"@call\": ...}}."
				)),
				_ => {}
			}
			for (key, inner) in map {
				let inner_at = format!("{at}.{key}");
				// An event's context keys are names the author chose, so only its
				// values are DynamicValues.
				if key == "context" && at.ends_with(".event") {
					if let Some(context) = inner.as_object() {
						for (name, value) in context {
							check_dynamic_values(value, &format!("{inner_at}.{name}"), problems);
						}
						continue;
					}
				}
				check_dynamic_values(inner, &inner_at, problems);
			}
		}
		Value::Array(items) => {
			for (i, inner) in items.iter().enumerate() {
				check_dynamic_values(inner, &format!("{at}[{i}]"), problems);
			}
		}
		_ => {}
	}
}

/// `@` followed by anything but another `@` is reserved by the v1.0 wire format.
fn is_reserved_key(key: &str) -> bool {
	key.strip_prefix('@').is_some_and(|rest| !rest.starts_with('@'))
}

/// Button events of an elicitation surface. `elicitation_result` turns them into the MCP action.
pub const ELICITATION_ACCEPT: &str = "accept";
pub const ELICITATION_DECLINE: &str = "decline";

/// The A2UI messages that show one MCP `elicitation/create` request from `server`: who asks and
/// its message, a bound field per `requestedSchema` property (form mode) or the link to open (URL
/// mode), and the two buttons.
/// Field values live at `/<property>` in the data model (the path escaped per RFC 6901, the data
/// model key raw), which `elicitation_result` reads back. The caller names the surface so two
/// open forms never share one.
pub fn elicitation_surface(surface_id: &str, server: &str, params: &Value) -> Value {
	let message = params["message"].as_str().unwrap_or_default();
	let url_mode = params["mode"] == "url";
	let mut shown = format!("**{server}** asks: {message}");
	let mut components = Vec::new();
	let mut data_model = serde_json::Map::new();
	let mut body = vec!["message".to_string()];
	if url_mode {
		if let Some(url) = params["url"].as_str() {
			shown = format!("{shown}\n\n[{url}]({url})");
		}
	} else if let Some(properties) = params["requestedSchema"]["properties"].as_object() {
		for (index, (name, property)) in properties.iter().enumerate() {
			let id = format!("field_{index}");
			components.push(elicitation_field(&id, name, property));
			if let Some(initial) = elicitation_initial(property) {
				data_model.insert(name.clone(), initial);
			}
			body.push(id);
		}
	}
	body.push("actions".to_string());
	let accept_label = if url_mode { "Done" } else { "Submit" };
	components.extend([
		json!({"id": "root", "component": "Card", "child": "body"}),
		json!({"id": "body", "component": "Column", "children": body}),
		json!({"id": "message", "component": "Text", "text": shown}),
		json!({"id": "actions", "component": "Row", "children": ["accept", "decline"]}),
		json!({"id": "accept_label", "component": "Text", "text": accept_label}),
		json!({"id": "accept", "component": "Button", "child": "accept_label",
			"action": {"event": {"name": ELICITATION_ACCEPT}}}),
		json!({"id": "decline_label", "component": "Text", "text": "Decline"}),
		json!({"id": "decline", "component": "Button", "child": "decline_label",
			"action": {"event": {"name": ELICITATION_DECLINE}}}),
	]);
	json!([{
		"version": PROTOCOL_VERSION,
		"createSurface": {
			"surfaceId": surface_id,
			"catalogId": BASIC_CATALOG_ID,
			"sendDataModel": true,
			"components": components,
			"dataModel": data_model,
		}
	}])
}

fn elicitation_field(id: &str, name: &str, property: &Value) -> Value {
	let title = property["title"].as_str().unwrap_or(name);
	let label = match property["description"].as_str() {
		Some(description) => format!("{title} — {description}"),
		None => title.to_string(),
	};
	// A property name may hold `/` or `~`, which are structure in a JSON Pointer.
	let value = json!({"@path": format!("/{}", name.replace('~', "~0").replace('/', "~1"))});
	let kind = property["type"].as_str();
	if kind == Some("boolean") {
		return json!({"id": id, "component": "CheckBox", "label": label, "value": value});
	}
	if let (Some("string"), Some(choices)) = (kind, property["enum"].as_array()) {
		let options: Vec<Value> = choices
			.iter()
			.filter_map(Value::as_str)
			.map(|choice| json!({"label": choice, "value": choice}))
			.collect();
		return json!({"id": id, "component": "ChoicePicker", "label": label, "options": options,
			"variant": "mutuallyExclusive", "value": value});
	}
	let variant = if matches!(kind, Some("number" | "integer")) {
		"number"
	} else {
		"shortText"
	};
	json!({"id": id, "component": "TextField", "label": label, "variant": variant, "value": value})
}

/// What a field shows before the user touches it. A checkbox always has a value; a picker binds an array.
fn elicitation_initial(property: &Value) -> Option<Value> {
	if property["type"] == "boolean" {
		return Some(json!(property["default"].as_bool().unwrap_or(false)));
	}
	let default = property.get("default")?;
	if property["enum"].is_array() {
		return Some(json!([default]));
	}
	Some(default.clone())
}

/// The MCP answer for a clicked elicitation surface. Only `accept` carries content, and only
/// properties the schema asks for: a blank or unparsable field is left out so the server's own
/// validation says what is missing.
pub fn elicitation_result(schema: &Value, action: &str, data_model: &Value) -> Value {
	if action != ELICITATION_ACCEPT {
		return json!({"action": action});
	}
	let mut content = serde_json::Map::new();
	if let Some(properties) = schema["properties"].as_object() {
		for (name, property) in properties {
			if let Some(value) = data_model.get(name).and_then(|raw| elicitation_value(property, raw)) {
				content.insert(name.clone(), value);
			}
		}
	}
	json!({"action": ELICITATION_ACCEPT, "content": content})
}

fn elicitation_value(property: &Value, raw: &Value) -> Option<Value> {
	// A number field writes a JSON number, but a text edit leaves a string behind.
	let text = || raw.as_str().map(str::trim).filter(|t| !t.is_empty());
	match property["type"].as_str() {
		Some("boolean") => raw.as_bool().map(Value::Bool),
		Some("integer") => match raw.as_i64() {
			Some(n) => Some(json!(n)),
			None => text()?.parse::<i64>().ok().map(|n| json!(n)),
		},
		Some("number") => match raw.as_f64() {
			Some(n) => Some(json!(n)),
			None => text()?
				.parse::<f64>()
				.ok()
				.and_then(serde_json::Number::from_f64)
				.map(Value::Number),
		},
		// A picker binds a one-element array, even when it is mutually exclusive.
		_ => match raw.as_array() {
			Some(picked) => picked.first().and_then(Value::as_str).map(|s| json!(s)),
			None => raw.as_str().filter(|s| !s.is_empty()).map(|s| json!(s)),
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_later_definition_replaces_the_children_the_earlier_one_listed() {
		let messages = vec![
			json!({"createSurface": {"surfaceId": "s1", "components": [
				{"id": "root", "component": "Column", "children": ["a"]},
				{"id": "a", "component": "Text", "text": "a"}
			]}}),
			json!({"updateComponents": {"surfaceId": "s1", "components": [
				{"id": "root", "component": "Column", "children": ["a"]}
			]}}),
		];
		assert_eq!(validate(&messages), Vec::<String>::new());
	}

	#[test]
	fn two_lists_may_share_one_template() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Column", "children": ["first", "second"]},
			{"id": "first", "component": "List", "children": {"componentId": "row", "path": "/a"}},
			{"id": "second", "component": "List", "children": {"componentId": "row", "path": "/b"}},
			{"id": "row", "component": "Text", "text": {"@path": "name"}}
		]}})];
		assert_eq!(validate(&messages), Vec::<String>::new());
	}

	/// One createSurface carrying a complete, valid tree.
	fn good() -> Vec<Value> {
		vec![json!({
			"version": "1.0",
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
	fn normalize_stamps_send_data_model_and_honours_an_explicit_false() {
		let mut messages = vec![
			json!({"createSurface": {"surfaceId": "s1"}}),
			json!({"createSurface": {"surfaceId": "s2", "sendDataModel": false}}),
		];
		normalize(&mut messages);
		assert_eq!(messages[0]["createSurface"]["sendDataModel"], true);
		assert_eq!(messages[1]["createSurface"]["sendDataModel"], false);
	}

	#[test]
	fn a_catalog_other_than_the_basic_one_is_reported() {
		let surface = validate(&[json!({"createSurface": {"surfaceId": "s1", "catalogId": "acme.com:custom"}})]);
		assert!(
			surface.iter().any(|p| p.contains("createSurface.catalogId")),
			"{surface:?}"
		);

		let component = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "a", "component": "Text", "text": "hi", "catalogId": "acme.com:custom"}
		]}})]);
		assert!(
			component.iter().any(|p| p.contains("components[0].catalogId")),
			"{component:?}"
		);

		let call = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "a", "component": "Text",
			 "text": {"@call": "required", "catalogId": "acme.com:custom", "args": {}}}
		]}})]);
		assert!(
			call.iter().any(|p| p.contains("components[0].text.catalogId")),
			"{call:?}"
		);

		let renderer_call = validate(&[json!({"callRendererFunction": {"functionCallId": "c1",
			"callFunction": {"@call": "required", "catalogId": "acme.com:custom"}}})]);
		assert!(
			renderer_call.iter().any(|p| p.contains("callFunction.catalogId")),
			"{renderer_call:?}"
		);

		// The basic catalog named explicitly is fine, and normalize still fills a missing one.
		let mut named = vec![json!({"createSurface": {"surfaceId": "s1", "catalogId": BASIC_CATALOG_ID}})];
		assert_eq!(validate(&named), Vec::<String>::new());
		normalize(&mut named);
		assert_eq!(named[0]["createSurface"]["catalogId"], BASIC_CATALOG_ID);
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
			json!({"version": "1.0", "createSurface": {"surfaceId": "s1"}}),
			json!({"version": "1.0", "updateComponents": {"surfaceId": "s1", "components": [
				{"id": "root", "component": "Text", "text": "hi"}
			]}}),
			json!({"version": "1.0", "updateDataModel": {"surfaceId": "s1", "path": "/a", "value": 1}}),
			json!({"version": "1.0", "callRendererFunction": {
				"functionCallId": "c1",
				"callFunction": {"@call": "openUrl", "catalogId": BASIC_CATALOG_ID,
								 "args": {"url": "https://example.com"}}
			}}),
			json!({"version": "1.0", "agentFunctionResponse": {"functionCallId": "c1", "value": 7}}),
			json!({"version": "1.0", "deleteSurface": {"surfaceId": "s1"}}),
		];
		assert_eq!(validate(&messages), Vec::<String>::new());
	}

	#[test]
	fn a_message_must_carry_exactly_one_kind() {
		let none = validate(&[json!({"version": "1.0"})]);
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
		assert!(problems.iter().any(|p| p.contains("surfaceId")), "{problems:?}");
	}

	#[test]
	fn update_data_model_needs_a_value_and_a_pointer_path() {
		let missing = validate(&[json!({"updateDataModel": {"surfaceId": "s1"}})]);
		assert!(missing.iter().any(|p| p.contains("\"value\"")), "{missing:?}");

		let bad_path = validate(&[json!({"updateDataModel": {"surfaceId": "s1", "path": "form/x", "value": 1}})]);
		assert!(bad_path.iter().any(|p| p.contains("JSON Pointer")), "{bad_path:?}");
	}

	#[test]
	fn a_null_value_is_a_delete_not_a_missing_value() {
		let messages = vec![json!({"updateDataModel": {"surfaceId": "s1", "path": "/a", "value": null}})];
		assert_eq!(validate(&messages), Vec::<String>::new());
	}

	#[test]
	fn an_unknown_component_names_the_catalog() {
		let messages = vec![json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Accordion"}
		]}})];
		let problems = validate(&messages);
		assert!(problems[0].contains("Accordion"), "{problems:?}");
		assert!(problems[0].contains("TextField"), "{problems:?}");
	}

	#[test]
	fn type_instead_of_component_gets_a_pointed_hint() {
		let messages = vec![json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "type": "Card"}
		]}})];
		let problems = validate(&messages);
		assert!(problems[0].contains("not \"type\""), "{problems:?}");
	}

	#[test]
	fn the_reserved_surface_component_is_rejected() {
		let messages = vec![json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Surface", "child": "x"}
		]}})];
		let problems = validate(&messages);
		assert!(problems[0].contains("reserved"), "{problems:?}");
	}

	#[test]
	fn components_outside_the_catalog_are_unknown() {
		for name in ["Heading", "Markdown", "Spacer"] {
			let messages = vec![json!({"updateComponents": {"surfaceId": "s1", "components": [
				{"id": "root", "component": name, "text": "Hi"}
			]}})];
			let problems = validate(&messages);
			assert!(problems[0].contains("unknown component"), "{problems:?}");
		}
	}

	#[test]
	fn a_missing_required_prop_is_reported_by_path() {
		let problems = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Slider", "value": 1}
		]}})]);
		assert_eq!(problems.len(), 1, "{problems:?}");
		assert!(problems[0].contains("updateComponents.components[0]"), "{problems:?}");
		assert!(problems[0].contains("missing required \"max\""), "{problems:?}");
	}

	#[test]
	fn an_unknown_prop_names_the_ones_the_component_has() {
		let problems = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Column", "children": [], "gap": 8}
		]}})]);
		assert_eq!(problems.len(), 1, "{problems:?}");
		assert!(
			problems[0].contains(".components[0].gap is not a Column prop"),
			"{problems:?}"
		);
		assert!(
			problems[0].contains("justify") && problems[0].contains("align"),
			"{problems:?}"
		);
	}

	#[test]
	fn checks_are_allowed_only_on_checkable_components() {
		let checks = json!([{"condition": true, "message": "m"}]);
		let ok = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "CheckBox", "label": "l", "value": false, "checks": checks}
		]}})]);
		assert_eq!(ok, Vec::<String>::new());
		let bad = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Text", "text": "t", "checks": checks}
		]}})]);
		assert!(bad.iter().any(|p| p.contains(".checks is not a Text prop")), "{bad:?}");
	}

	#[test]
	fn a_literal_outside_the_enum_is_reported_and_a_binding_is_not() {
		let bad = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Button", "child": "x", "variant": "danger",
			 "action": {"event": {"name": "go"}}}
		]}})]);
		assert_eq!(bad.len(), 1, "{bad:?}");
		assert!(
			bad[0].contains(".variant \"danger\" is not one of: default, primary, borderless"),
			"{bad:?}"
		);
		let bound = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Text", "text": "t", "variant": {"@path": "/v"}}
		]}})]);
		assert_eq!(bound, Vec::<String>::new());
	}

	#[test]
	fn a_call_names_a_catalog_function_or_index() {
		let bad = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Text", "text": {"@call": "upper", "args": {}}}
		]}})]);
		assert!(
			bad.iter().any(|p| p.contains("\"upper\" is not a catalog function")),
			"{bad:?}"
		);
		let ok = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Text", "text": {"@call": "@index"}}
		]}})]);
		assert_eq!(ok, Vec::<String>::new());
	}

	#[test]
	fn the_content_spelling_of_a_tab_is_rejected() {
		let problems = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Tabs", "tabs": [{"title": "One", "content": "p1"}]}
		]}})]);
		assert!(
			problems.iter().any(|p| p.contains("tabs[0] needs a \"child\"")),
			"{problems:?}"
		);
	}

	#[test]
	fn the_actions_and_open_url_button_spellings_are_rejected() {
		for button in [
			json!({"id": "root", "component": "Button", "child": "x", "actions": {"event": {"name": "n"}}}),
			json!({"id": "root", "component": "Button", "child": "x", "action": {"openUrl": {"url": "https://x.test"}}}),
		] {
			let problems = validate(&[json!({"createSurface": {"surfaceId": "s1", "components": [
				button,
				{"id": "x", "component": "Text", "text": "Go"}
			]}})]);
			assert!(
				problems
					.iter()
					.any(|p| p.contains("missing required \"action\"") || p.contains("no event name")),
				"{problems:?}"
			);
		}
	}

	#[test]
	fn a_new_surface_must_declare_root() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "body", "component": "Text", "text": "hi"}
		]}})];
		let problems = validate(&messages);
		assert!(problems.iter().any(|p| p.contains("\"root\"")), "{problems:?}");
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
		assert!(problems.iter().any(|p| p.contains("\"gone\"")), "{problems:?}");
	}

	#[test]
	fn incremental_updates_may_reference_earlier_components() {
		// No createSurface: the ids live on a surface a previous call built.
		let messages = vec![json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "row", "component": "Row", "children": ["built", "earlier"]}
		]}})];
		assert_eq!(validate(&messages), Vec::<String>::new());
	}

	#[test]
	fn inline_children_are_rejected() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Column",
			 "children": [{"id": "nested", "component": "Text", "text": "no"}]}
		]}})];
		let problems = validate(&messages);
		assert!(problems.iter().any(|p| p.contains("inline")), "{problems:?}");
	}

	#[test]
	fn a_child_list_template_needs_a_component_id_and_path() {
		let ok = validate(&[json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "List", "children": {"componentId": "row", "path": "/items"}},
			{"id": "row", "component": "Text", "text": {"@path": "name"}}
		]}})]);
		assert_eq!(ok, Vec::<String>::new());

		let bad = validate(&[json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "List", "children": {"componentId": "row"}},
			{"id": "row", "component": "Text", "text": "x"}
		]}})]);
		assert!(bad.iter().any(|p| p.contains("\"path\"")), "{bad:?}");
	}

	#[test]
	fn tabs_children_are_resolved_through_child() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Tabs", "tabs": [
				{"title": "One", "child": "p1"},
				{"title": "Two", "child": "p2"}
			]},
			{"id": "p1", "component": "Text", "text": "1"},
			{"id": "p2", "component": "Text", "text": "2"}
		]}})];
		assert_eq!(validate(&messages), Vec::<String>::new());
	}

	#[test]
	fn a_reserved_at_key_is_reported_and_double_at_is_not() {
		let bad = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Text", "text": {"nested": [{"@bogus": 1}]}}
		]}})]);
		assert!(
			bad.iter()
				.any(|p| p.contains("reserved key \"@bogus\"") && p.contains("@@")),
			"{bad:?}"
		);

		let ok = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Text", "text": {"@@type": 1, "v": {"@path": "/a"}}}
		]}})]);
		assert_eq!(ok, Vec::<String>::new());
	}

	#[test]
	fn a_binding_or_call_without_its_at_is_reported() {
		let problems = validate(&[json!({"updateComponents": {"surfaceId": "s1", "components": [
			{"id": "a", "component": "Text", "text": {"path": "/x"}},
			{"id": "b", "component": "Text", "text": {"call": "required"}},
			{"id": "c", "component": "Button", "child": "a",
			 "action": {"functionCall": {"call": "openUrl", "args": {}}}}
		]}})]);
		assert!(
			problems
				.iter()
				.any(|p| p.contains("[0].text") && p.contains("{\"@path\": ...}")),
			"{problems:?}"
		);
		assert!(
			problems
				.iter()
				.any(|p| p.contains("[1].text") && p.contains("{\"@call\": ...}")),
			"{problems:?}"
		);
		assert!(
			problems.iter().any(|p| p.contains("[2].action.functionCall")),
			"{problems:?}"
		);
	}

	#[test]
	fn a_tab_without_a_child_is_reported() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Tabs", "tabs": [{"title": "One"}]}
		]}})];
		let problems = validate(&messages);
		assert!(problems.iter().any(|p| p.contains("tabs[0]")), "{problems:?}");
	}

	#[test]
	fn a_function_call_needs_an_id_a_name_and_a_catalog() {
		let problems = validate(&[json!({"callRendererFunction": {"callFunction": {}}})]);
		assert!(problems.iter().any(|p| p.contains("functionCallId")), "{problems:?}");
		assert!(problems.iter().any(|p| p.contains("\"@call\"")), "{problems:?}");
		assert!(problems.iter().any(|p| p.contains("\"catalogId\"")), "{problems:?}");
	}

	#[test]
	fn every_renderer_function_call_is_answered_with_invalid_function_call() {
		let mut messages = vec![
			json!({"version": "1.0", "callRendererFunction": {"functionCallId": "c1",
				"callFunction": {"@call": "openUrl", "catalogId": BASIC_CATALOG_ID,
					"args": {"url": "https://example.com"}}}}),
			json!({"version": "1.0", "updateDataModel": {"surfaceId": "s1", "path": "/a", "value": 1}}),
			json!({"version": "1.0", "callRendererFunction": {"functionCallId": "c2",
				"callFunction": {"@call": "required", "catalogId": BASIC_CATALOG_ID}}}),
		];
		let responses = reject_renderer_calls(&mut messages);
		assert_eq!(messages.len(), 1);
		assert!(messages[0].get("updateDataModel").is_some());
		assert_eq!(responses.len(), 2);
		for (response, id) in responses.iter().zip(["c1", "c2"]) {
			assert_eq!(response["version"], "1.0");
			let body = &response["rendererFunctionResponse"];
			assert_eq!(body["functionCallId"], id);
			assert_eq!(body["error"]["code"], "INVALID_FUNCTION_CALL");
			assert!(body["error"]["message"].as_str().is_some_and(|m| !m.is_empty()));
		}

		// An envelope of nothing but calls leaves nothing to paint.
		let mut only_calls = vec![messages_call()];
		assert_eq!(reject_renderer_calls(&mut only_calls).len(), 1);
		assert!(only_calls.is_empty());
	}

	fn messages_call() -> Value {
		json!({"version": "1.0", "callRendererFunction": {"functionCallId": "c9",
			"callFunction": {"@call": "openUrl", "catalogId": BASIC_CATALOG_ID}}})
	}

	#[test]
	fn a_wait_with_a_function_call_still_needs_a_button_that_sends_an_event() {
		let awaited = vec!["pick".to_string()];
		let messages = vec![
			json!({"createSurface": {"surfaceId": "s1", "components": [
				{"id": "root", "component": "Text", "text": "hi"}
			]}}),
			messages_call(),
		];
		assert!(unanswerable_wait(&messages, &awaited).is_some());
	}

	#[test]
	fn one_envelope_may_name_only_one_surface() {
		let problems = validate(&[
			json!({"createSurface": {"surfaceId": "a"}}),
			json!({"updateDataModel": {"surfaceId": "b", "path": "/x", "value": 1}}),
		]);
		assert!(
			problems.iter().any(|p| p.contains("one surface per render_ui call")),
			"{problems:?}"
		);
		let same = validate(&[
			json!({"createSurface": {"surfaceId": "a"}}),
			json!({"updateDataModel": {"surfaceId": "a", "path": "/x", "value": 1}}),
		]);
		assert_eq!(same, Vec::<String>::new());
	}

	/// A saved render_ui body holding `messages`.
	fn saved(messages: Vec<Value>) -> Value {
		json!({"id": "u_1", "status": "resolved", "messages": messages})
	}

	fn create(id: &str) -> Value {
		json!({"version": "1.0", "createSurface": {"surfaceId": id}})
	}

	fn delete(id: &str) -> Value {
		json!({"version": "1.0", "deleteSurface": {"surfaceId": id}})
	}

	fn update_model(id: &str) -> Value {
		json!({"version": "1.0", "updateDataModel": {"surfaceId": id, "path": "/a", "value": 1}})
	}

	fn error_of(text: &str) -> Value {
		serde_json::from_str::<Value>(text).expect("error JSON")["error"].clone()
	}

	#[test]
	fn creating_a_live_surface_is_an_integrity_error() {
		let bodies = [saved(vec![create("s1")])];
		let text = surface_lifecycle_error(&bodies, &[create("s1")]).expect("rejected");
		let error = error_of(&text);
		assert_eq!(error["code"], "INTEGRITY_ERROR");
		assert_eq!(error["surfaceId"], "s1");
		assert!(error["message"].as_str().unwrap().contains("deleteSurface"));
		// Recreating after a delete, in the same envelope or an earlier call, is fine.
		assert_eq!(surface_lifecycle_error(&bodies, &[delete("s1"), create("s1")]), None);
		let deleted = [saved(vec![create("s1")]), saved(vec![delete("s1")])];
		assert_eq!(surface_lifecycle_error(&deleted, &[create("s1")]), None);
	}

	#[test]
	fn updating_or_deleting_a_surface_that_is_not_live_fails_validation() {
		let none: [Value; 0] = [];
		for msg in [update_model("s1"), delete("s1")] {
			let text = surface_lifecycle_error(&none, &[msg]).expect("rejected");
			let error = error_of(&text);
			assert_eq!(error["code"], "VALIDATION_FAILED");
			assert!(error["message"].as_str().unwrap().contains("createSurface"));
		}
		// A surface created earlier in the same envelope is live for what follows.
		assert_eq!(
			surface_lifecycle_error(&none, &[create("s1"), update_model("s1")]),
			None
		);
		// ...and one deleted earlier in it is not.
		let bodies = [saved(vec![create("s1")])];
		assert!(surface_lifecycle_error(&bodies, &[delete("s1"), update_model("s1")]).is_some());
	}

	#[test]
	fn only_v1_messages_in_saved_bodies_count_toward_live_surfaces() {
		let legacy = json!({"createSurface": {"surfaceId": "s1"}});
		let bodies = [saved(vec![legacy])];
		assert!(surface_lifecycle_error(&bodies, &[update_model("s1")]).is_some());
		assert_eq!(surface_lifecycle_error(&bodies, &[create("s1")]), None);
	}

	#[test]
	fn validation_failures_become_spec_error_messages() {
		let messages = vec![
			json!({"version": "1.0", "updateComponents": {"surfaceId": "s1", "components": [
				{"id": "root", "component": "Nope"}
			]}}),
		];
		let problems = validate(&messages);
		let text = rejection_text(&problems, &messages);
		let (head, lines) = text.split_once('\n').expect("head and error lines");
		assert_eq!(head, "This A2UI envelope will not render:");
		let error = error_of(lines.lines().next().unwrap());
		assert_eq!(error["code"], "VALIDATION_FAILED");
		assert_eq!(error["surfaceId"], "s1");
		assert_eq!(error["path"], "/messages/0/updateComponents/components/0");
		assert_eq!(error["message"], problems[0]);

		// A problem about the envelope as a whole points at the root.
		let whole = rejection_text(&["This envelope is odd.".to_string()], &[]);
		let error = error_of(whole.lines().nth(1).unwrap());
		assert_eq!(error["path"], "/");
		assert_eq!(error["surfaceId"], "");
	}

	#[test]
	fn a_click_goes_to_the_agent_as_a_spec_action_message() {
		let resolution = json!({
			"name": "approve", "surfaceId": "s1", "sourceComponentId": "ok",
			"timestamp": "2026-01-01T00:00:00.000Z", "context": {"reason": "fine"},
			"userMessage": "Approved", "dataModel": {"form": {"reason": "fine"}}, "actor": "human",
		});
		let message = action_message(&resolution);
		assert_eq!(
			message,
			json!({
				"version": "1.0",
				"action": {"name": "approve", "surfaceId": "s1", "sourceComponentId": "ok",
					"timestamp": "2026-01-01T00:00:00.000Z", "context": {"reason": "fine"},
					"userMessage": "Approved"},
				"metadata": {"a2uiRendererDataModel": {"version": "1.0",
					"surfaces": {"s1": {"form": {"reason": "fine"}}}}},
			})
		);

		let without_model = json!({"name": "go", "surfaceId": "s1", "context": {}, "actor": "human"});
		let message = action_message(&without_model);
		assert!(message.get("metadata").is_none());
		assert!(message["action"].get("actor").is_none());
		assert!(message["action"].get("userMessage").is_none());
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
			("36_modal", include_str!("../tests/fixtures/a2ui/36_modal.json")),
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
		let messages = vec![json!({"createSurface": {"surfaceId": "pick", "components": [
			{"id": "root", "component": "Card", "child": "go"},
			{"id": "go", "component": "Button", "child": "label", "variant": "primary"},
			{"id": "label", "component": "Text", "text": "Open it"}
		]}})];
		let problems = validate(&messages);
		assert!(
			problems.iter().any(|p| p.contains("missing required \"action\"")),
			"{problems:?}"
		);
	}

	#[test]
	fn every_button_action_the_renderer_accepts_validates() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Row", "children": ["a", "b"]},
			{"id": "a", "component": "Button", "child": "ta", "action": {"event": {"name": "ok"}}},
			{"id": "b", "component": "Button", "child": "tb",
			 "action": {"functionCall": {"@call": "openUrl", "args": {"url": "https://x.test"}}}},
			{"id": "ta", "component": "Text", "text": "Go"},
			{"id": "tb", "component": "Text", "text": "Go"}
		]}})];
		assert_eq!(validate(&messages), Vec::<String>::new());
	}

	#[test]
	fn a_modal_trigger_button_needs_an_action_like_any_button() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Modal", "trigger": "open", "content": "body"},
			{"id": "open", "component": "Button", "child": "label"},
			{"id": "label", "component": "Text", "text": "Details"},
			{"id": "body", "component": "Text", "text": "More"}
		]}})];
		let problems = validate(&messages);
		assert!(
			problems.iter().any(|p| p.contains("missing required \"action\"")),
			"{problems:?}"
		);
	}

	#[test]
	fn a_component_no_parent_lists_is_reported() {
		// What an agent actually sent: the CheckBox a Button's checks read was
		// declared but never placed, so the user saw the check's toast and no
		// box to tick.
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Card", "child": "body"},
			{"id": "body", "component": "Column", "children": ["ok"]},
			{"id": "okLabel", "component": "Text", "text": "Post it"},
			{"id": "ok", "component": "Button", "child": "okLabel",
			 "action": {"event": {"name": "approve"}},
			 "checks": [{"condition": {"@path": "/form/ack"}, "message": "Tick it"}]},
			{"id": "ack", "component": "CheckBox", "label": "I read it",
			 "value": {"@path": "/form/ack"}}
		]}})];
		let problems = validate(&messages);
		assert_eq!(problems.len(), 1, "{problems:?}");
		assert!(problems[0].contains("\"ack\""), "{problems:?}");
	}

	#[test]
	fn a_component_with_two_parents_is_reported() {
		// The label Text drawn both beside its Button and inside it.
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Row", "children": ["keysLabel", "keys"]},
			{"id": "keysLabel", "component": "Text", "text": "Type it"},
			{"id": "keys", "component": "Button", "child": "keysLabel",
			 "action": {"event": {"name": "pick"}}}
		]}})];
		let problems = validate(&messages);
		assert_eq!(problems.len(), 1, "{problems:?}");
		assert!(problems[0].contains("\"keysLabel\""), "{problems:?}");
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
		let update = vec![json!({"updateDataModel": {"surfaceId": "s1", "path": "/a", "value": 1}})];
		assert_eq!(unanswerable_wait(&update, &awaited), None);
	}

	#[test]
	fn a_wait_on_a_deleted_surface_is_unanswerable() {
		let awaited = vec!["pick".to_string()];
		let delete = vec![json!({"deleteSurface": {"surfaceId": "s1"}})];
		let reason = unanswerable_wait(&delete, &awaited);
		assert!(reason.is_some_and(|r| r.contains("Drop await_events")));
		let recreate = vec![json!({"deleteSurface": {"surfaceId": "s1"}}), good()[0].clone()];
		assert_eq!(unanswerable_wait(&recreate, &awaited), None);
	}

	#[test]
	fn a_modal_trigger_button_does_not_answer_a_wait() {
		let awaited = vec!["pick".to_string()];
		let components = |extra: Value| {
			vec![json!({"createSurface": {"surfaceId": "s1", "components": [
				{"id": "root", "component": "Modal", "trigger": "open", "content": "body"},
				{"id": "openLabel", "component": "Text", "text": "Open"},
				{"id": "open", "component": "Button", "child": "openLabel",
				 "action": {"event": {"name": "pick"}}},
				{"id": "body", "component": "Text", "text": "hi"},
				extra
			]}})]
		};
		let only_trigger = components(json!({"id": "x", "component": "Text", "text": "x"}));
		assert!(unanswerable_wait(&only_trigger, &awaited).is_some());
		let with_other = components(json!({"id": "go", "component": "Button", "child": "openLabel",
			"action": {"event": {"name": "pick"}}}));
		assert_eq!(unanswerable_wait(&with_other, &awaited), None);
	}

	#[test]
	fn event_context_keys_are_names_not_bindings() {
		let messages = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Button", "child": "l",
			 "action": {"event": {"name": "save", "context": {"path": {"@path": "/f"}}}}},
			{"id": "l", "component": "Text", "text": "Save"}
		]}})];
		assert_eq!(validate(&messages), Vec::<String>::new());
		let stale = vec![json!({"createSurface": {"surfaceId": "s1", "components": [
			{"id": "root", "component": "Button", "child": "l",
			 "action": {"event": {"name": "save", "context": {"v": {"path": "/f"}}}}},
			{"id": "l", "component": "Text", "text": "Save"}
		]}})];
		assert_eq!(validate(&stale).len(), 1);
	}

	#[test]
	fn an_icon_name_outside_the_catalog_is_reported() {
		let icon = |name: Value| {
			validate(&[json!({"createSurface": {"surfaceId": "s1", "components": [
				{"id": "root", "component": "Icon", "name": name}
			]}})])
		};
		assert_eq!(icon(json!("warning")), Vec::<String>::new());
		assert_eq!(icon(json!({"@path": "/icon"})), Vec::<String>::new());
		let problems = icon(json!("rocket"));
		assert_eq!(problems.len(), 1, "{problems:?}");
		assert!(
			problems[0].contains("rocket") && problems[0].contains("shoppingCart"),
			"{problems:?}"
		);
	}

	#[test]
	fn an_elicitation_form_has_a_bound_field_per_property_and_validates() {
		let params = json!({
			"mode": "form",
			"message": "Which repo?",
			"requestedSchema": {"type": "object", "properties": {
				"repo": {"type": "string", "title": "Repository", "description": "owner/name"},
				"kind": {"type": "string", "enum": ["a", "b"], "default": "b"},
				"private": {"type": "boolean"},
				"stars": {"type": "integer"}
			}}
		});
		let messages = elicitation_surface("elicitation-call-1", "forms", &params);
		let messages = messages.as_array().unwrap();
		assert_eq!(validate(messages), Vec::<String>::new());
		let surface = &messages[0]["createSurface"];
		let components = surface["components"].as_array().unwrap();
		let by_kind = |kind: &str| components.iter().filter(|c| c["component"] == kind).count();
		assert_eq!(by_kind("TextField"), 2);
		assert_eq!(by_kind("ChoicePicker"), 1);
		assert_eq!(by_kind("CheckBox"), 1);
		assert_eq!(by_kind("Button"), 2);
		let repo = components.iter().find(|c| c["value"]["@path"] == "/repo").unwrap();
		assert_eq!(repo["label"], "Repository — owner/name");
		assert_eq!(surface["dataModel"], json!({"kind": ["b"], "private": false}));
		assert_eq!(surface["sendDataModel"], true);
		assert_eq!(surface["surfaceId"], "elicitation-call-1");
		let submit = components.iter().find(|c| c["id"] == "accept_label").unwrap();
		assert_eq!(submit["text"], "Submit");
	}

	#[test]
	fn an_elicitation_url_shows_the_link_and_asks_for_done() {
		let params = json!({"mode": "url", "message": "Sign in", "url": "https://example.com/auth"});
		let messages = elicitation_surface("elicitation-call-2", "auth", &params);
		let messages = messages.as_array().unwrap();
		assert_eq!(validate(messages), Vec::<String>::new());
		let components = messages[0]["createSurface"]["components"].as_array().unwrap();
		let shown = components.iter().find(|c| c["id"] == "message").unwrap();
		assert!(shown["text"]
			.as_str()
			.unwrap()
			.contains("[https://example.com/auth](https://example.com/auth)"));
		assert!(components.iter().all(|c| c["component"] != "TextField"));
		let done = components.iter().find(|c| c["id"] == "accept_label").unwrap();
		assert_eq!(done["text"], "Done");
	}

	#[test]
	fn a_property_name_with_pointer_characters_is_escaped_in_its_path_only() {
		let properties = json!({
			"a/b": {"type": "string", "default": "x"},
			"c~d": {"type": "string"}
		});
		let params = json!({"message": "Where?", "requestedSchema": {"properties": properties}});
		let messages = elicitation_surface("elicitation-call-3", "forms", &params);
		let surface = &messages[0]["createSurface"];
		let components = surface["components"].as_array().unwrap();
		assert!(components.iter().any(|c| c["value"]["@path"] == "/a~1b"));
		assert!(components.iter().any(|c| c["value"]["@path"] == "/c~0d"));
		assert_eq!(surface["dataModel"], json!({"a/b": "x"}));
		let result = elicitation_result(&json!({"properties": properties}), "accept", &json!({"a/b": "y"}));
		assert_eq!(result, json!({"action": "accept", "content": {"a/b": "y"}}));
	}

	#[test]
	fn an_accepted_elicitation_carries_typed_schema_properties_only() {
		let schema = json!({"properties": {
			"name": {"type": "string"},
			"kind": {"type": "string", "enum": ["a", "b"]},
			"count": {"type": "integer"},
			"ratio": {"type": "number"},
			"flag": {"type": "boolean"},
			"blank": {"type": "string"},
			"bad": {"type": "integer"}
		}});
		let model = json!({
			"name": "octoweb", "kind": ["b"], "count": " 7 ", "ratio": 2.5, "flag": true,
			"blank": "", "bad": "seven", "stray": "ignored"
		});
		let result = elicitation_result(&schema, "accept", &model);
		assert_eq!(
			result,
			json!({"action": "accept", "content": {
				"name": "octoweb", "kind": "b", "count": 7, "ratio": 2.5, "flag": true
			}})
		);
	}

	#[test]
	fn a_declined_elicitation_carries_no_content() {
		let schema = json!({"properties": {"name": {"type": "string"}}});
		let model = json!({"name": "octoweb"});
		assert_eq!(
			elicitation_result(&schema, "decline", &model),
			json!({"action": "decline"})
		);
	}
}
