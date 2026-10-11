//! Generate libevdev's event-name tables from its pinned Linux headers without
//! requiring the upstream Python generator on the build host.
use std::collections::BTreeMap;

const PREFIXES: &[&str] = &[
    "EV",
    "REL",
    "ABS",
    "KEY",
    "BTN",
    "LED",
    "SND",
    "MSC",
    "SW",
    "FF",
    "SYN",
    "REP",
    "INPUT_PROP",
    "MT_TOOL",
];
const DUPLICATES: &[&str] = &[
    "EV_VERSION",
    "BTN_MISC",
    "BTN_MOUSE",
    "BTN_JOYSTICK",
    "BTN_GAMEPAD",
    "BTN_DIGI",
    "BTN_WHEEL",
    "BTN_TRIGGER_HAPPY",
    "SW_MAX",
    "REP_MAX",
];

fn definition(line: &str) -> Option<(&str, u32)> {
    let mut fields = line.split_whitespace();
    if fields.next()? != "#define" {
        return None;
    }
    let name = fields.next()?;
    let value = fields.next()?;
    let number = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()?
    } else {
        value.parse().ok()?
    };
    Some((name, number))
}

fn group<'a>(definitions: &[(&'a str, u32)], prefix: &str) -> BTreeMap<u32, &'a str> {
    let prefix = format!("{prefix}_");
    definitions
        .iter()
        .filter(|(name, _)| name.starts_with(&prefix) && !DUPLICATES.contains(name))
        .map(|(name, value)| (*value, *name))
        .collect()
}

fn entries(names: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut names: Vec<_> = names.into_iter().collect();
    names.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
    names
        .iter()
        .map(|name| {
            let name = name.as_ref();
            format!("    {{ .name = \"{name}\", .value = {name} }},\n")
        })
        .collect()
}

pub fn generate(headers: &str) -> String {
    let definitions: Vec<_> = headers.lines().filter_map(definition).collect();
    let groups: BTreeMap<_, _> = PREFIXES
        .iter()
        .copied()
        .map(|prefix| (prefix, group(&definitions, prefix)))
        .collect();
    let maps: String = PREFIXES
        .iter()
        .copied()
        .filter(|prefix| *prefix != "BTN")
        .map(|prefix| {
            let name = prefix.to_lowercase();
            let values: String = groups[prefix]
                .values()
                .chain(
                    (prefix == "KEY")
                        .then_some(&groups["BTN"])
                        .into_iter()
                        .flat_map(|g| g.values()),
                )
                .map(|name| format!("    [{name}] = \"{name}\",\n"))
                .collect();
            format!("static const char * const {name}_map[{prefix}_MAX + 1] = {{\n{values}}};\n\n")
        })
        .collect();
    let event_maps: String = PREFIXES
        .iter()
        .copied()
        .filter(|prefix| !["BTN", "EV", "INPUT_PROP", "MT_TOOL"].contains(prefix))
        .map(|prefix| format!("    [EV_{prefix}] = {}_map,\n", prefix.to_lowercase()))
        .collect();
    let max_event = definitions
        .iter()
        .find(|(name, _)| *name == "EV_MAX")
        .expect("EV_MAX in pinned Linux headers")
        .1;
    let maxima: String = (0..=max_event)
        .map(|value| {
            match groups["EV"]
                .get(&value)
                .and_then(|name| name.strip_prefix("EV_"))
            {
                Some(prefix) if PREFIXES.contains(&prefix) => format!("    {prefix}_MAX,\n"),
                _ => "    -1,\n".to_owned(),
            }
        })
        .collect();
    let mut code_prefixes: Vec<_> = PREFIXES
        .iter()
        .copied()
        .filter(|prefix| !["EV", "INPUT_PROP", "MT_TOOL"].contains(prefix))
        .collect();
    code_prefixes.sort();
    let code_names: String = code_prefixes
        .iter()
        .map(|prefix| {
            let additional: &[&str] = match *prefix {
                "BTN" => &["BTN_A", "BTN_B", "BTN_X", "BTN_Y"],
                "SW" => &["SW_MAX"],
                "REP" => &["REP_MAX"],
                _ => &[],
            };
            entries(
                groups[*prefix]
                    .values()
                    .copied()
                    .chain(additional.iter().copied()),
            )
        })
        .collect();
    let tool_names = entries(groups["MT_TOOL"].values().copied());
    let ev_names = entries(groups["EV"].values().copied());
    let prop_names = entries(groups["INPUT_PROP"].values().copied());
    format!(
        "/* THIS FILE IS GENERATED, DO NOT EDIT */\n\n\
#ifndef EVENT_NAMES_H\n#define EVENT_NAMES_H\n\n\
{maps}static const char * const * const event_type_map[EV_MAX + 1] = {{\n{event_maps}}};\n\n\
#if __clang__\n#pragma clang diagnostic push\n#pragma clang diagnostic ignored \"-Winitializer-overrides\"\n\
#elif __GNUC__\n#pragma GCC diagnostic push\n#pragma GCC diagnostic ignored \"-Woverride-init\"\n#endif\n\
static const int ev_max[EV_MAX + 1] = {{\n{maxima}}};\n\
#if __clang__\n#pragma clang diagnostic pop /* \"-Winitializer-overrides\" */\n\
#elif __GNUC__\n#pragma GCC diagnostic pop /* \"-Woverride-init\" */\n#endif\n\n\
struct name_entry {{\n    const char *name;\n    unsigned int value;\n}};\n\n\
static const struct name_entry tool_type_names[] = {{\n{tool_names}}};\n\n\
static const struct name_entry ev_names[] = {{\n{ev_names}}};\n\n\
static const struct name_entry code_names[] = {{\n{code_names}}};\n\n\
static const struct name_entry prop_names[] = {{\n{prop_names}}};\n\n\
#endif /* EVENT_NAMES_H */\n"
    )
}
