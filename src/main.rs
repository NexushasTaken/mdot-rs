use colored::*;
use dirs;
use log::{error, info, warn};
use mlua::prelude::*;
use mlua::{Function, Lua, Table, TablePairs, Value};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf}; // 1. Import the Colorize trait

// alias Command string
// alias HookAction Command | fun() | (Command | fun())[]
//
// alias OSPackageName string | table<string, string>
// alias PathString string
// alias TargetList PathString | PathString[]
//
// class LinkObject
// field source? PathString
// field targets? TargetList
// field overwrite? boolean
// field backup? boolean
//
// alias LinkEntrySpec LinkObject | table<PathString, TargetList>
// alias LinksArraySpec LinkEntrySpec[]
//
// class PackageSchema
// field name? string
// field package_name? OSPackageName
// field enabled? boolean | fun(): boolean
// field depends? PackageList
// field links? LinksArraySpec
// field excludes? TargetList
// field templates? TargetList
// field default_target? PathString
// field on_install? HookAction
// field on_deploy? HookAction
//
// alias PackageItemSpec string | PackageSchema
// alias PackageList PackageItemSpec[]

const APP_NAME: &str = "mdot";

macro_rules! fatal {
    ($($arg:tt)*) => {{
        log::error!($($arg)*);
        std::process::exit(1);
    }};
}

fn lua_value_to_str(val: &Value) -> String {
    match val {
        Value::String(_) => val
            .to_string()
            .map_err(|_| fatal!("Field contains invalid UTF-8 bytes"))
            .unwrap()
            .to_string(),
        _ => fatal!("expected type 'String', got {:#?}", val),
    }
}

fn lua_str_to_str(val: &mlua::String) -> String {
    val.to_str()
        .map_err(|_| fatal!("Field contains invalid UTF-8 bytes"))
        .unwrap()
        .to_string()
}

type OSPackage = HashMap<String, String>;
#[derive(Debug, PartialEq, Clone)]
enum OSPackageName {
    AsPackage(bool),
    Name(String),
    Package(OSPackage),
}

#[derive(Debug, PartialEq, Clone)]
enum Enabled {
    Enable(bool),
    Hook(Function),
}

impl Default for Enabled {
    fn default() -> Self {
        Enabled::Enable(true)
    }
}

#[derive(Debug, PartialEq, Clone)]
struct LinkObject {
    source: PathBuf,
    targets: Vec<PathBuf>,
    overwrite: bool,
    backup: bool,
}

#[derive(Default, Debug, PartialEq, Clone)]
struct Package {
    name: String,
    package_name: Option<OSPackageName>,
    // enabled: bool,
    enabled: Enabled,
    depends: Vec<Package>,
    links: Vec<LinkObject>,
    excludes: Vec<PathBuf>,
    templates: Vec<PathBuf>,
}

impl Package {
    fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    fn has_name(tbl: &Table) -> bool {
        return tbl.get::<String>(1).is_ok() || tbl.get::<String>("name").is_ok();
    }

    fn extract_name(tbl: &Table) -> Result<String, String> {
        let idx_1: Option<String> = tbl.get(1).ok();
        let name_key: Option<String> = tbl.get("name").ok();

        match (idx_1, name_key) {
            (Some(_), Some(_)) => Err("provide 'name' OR [1] but not both.".to_string()),
            (Some(name), None) | (None, Some(name)) => Ok(name),
            (None, None) => {
                Err("package must have a name (at index [1] or as 'name' field)".to_string())
            }
        }
    }

    fn parse_target_list(targets: Value) -> Vec<PathBuf> {
        match targets {
            Value::String(target) => vec![PathBuf::from(lua_str_to_str(&target))],
            Value::Table(target_list) => {
                let mut links: Vec<PathBuf> = Vec::new();
                for pair in target_list.pairs::<Value, Value>() {
                    match pair.unwrap() {
                        (Value::Integer(_), Value::String(target)) => {
                            links.push(PathBuf::from(lua_str_to_str(&target)));
                        }
                        (k, v) => {
                            fatal!("Link invalid target element: [{:#?}] = {:#?}", k, v);
                        }
                    }
                }
                links
            }
            v => fatal!(
                "Link 'targets' expected type 'String' or 'Table', got {:?}",
                v
            ),
        }
    }

    fn extract_links(tbl: &Table) -> Vec<LinkObject> {
        tbl.pairs::<Value, Value>()
            .map(|pair| {
                let (key, value): (Value, Value) = pair.unwrap();
                match (key, value) {
                    (Value::Integer(_), Value::Table(tbl)) => {
                        let source: String = match tbl.get("source").unwrap() {
                            Value::String(s) => lua_str_to_str(&s),
                            Value::Nil => fatal!("Link must contain 'source'"),
                            v => fatal!("Link 'source' expected type 'String', got {:?}", v),
                        };
                        let targets = match tbl.get("targets").unwrap() {
                            Value::Nil => fatal!("Link must contain 'targets'"),
                            v => Package::parse_target_list(v),
                        };
                        let overwrite = match tbl.get("overwrite").unwrap() {
                            Value::Boolean(v) => v,
                            Value::Nil => false,
                            v => fatal!("Link 'overwrite' expected type 'Boolean', got {:?}", v),
                        };
                        let backup = match tbl.get("backup").unwrap() {
                            Value::Boolean(v) => v,
                            Value::Nil => false,
                            v => fatal!("Link 'backup' expected type 'Boolean', got {:?}", v),
                        };
                        LinkObject {
                            source: PathBuf::from(source),
                            targets: targets,
                            overwrite: overwrite,
                            backup: backup,
                        }
                    }
                    (Value::String(source), v) => LinkObject {
                        source: PathBuf::from(lua_str_to_str(&source)),
                        targets: Package::parse_target_list(v),
                        overwrite: false,
                        backup: false,
                    },
                    (key, value) => {
                        fatal!("expected Link element, found {:#?} = {:#?}", key, value);
                    }
                }
            })
            .collect()
    }

    fn extract_targets(value: &Value) -> Vec<PathBuf> {
        match value {
            Value::String(target) => {
                return vec![PathBuf::from(lua_value_to_str(value))];
            }
            Value::Table(targets) => {
                return targets
                    .sequence_values::<Value>()
                    .map(|v| match v.clone().unwrap() {
                        Value::String(target) => PathBuf::from(lua_str_to_str(&target)),
                        _ => {
                            fatal!("expected 'String', found {:#?}", v);
                        }
                    })
                    .collect();
            }
            _ => {
                fatal!("expected 'String' or 'Table', found {:#?}", value);
            }
        }
        Vec::new()
    }

    fn from_table(name: Option<String>, tbl: &Table) -> Self {
        // todo!(); // Table -> Package
        let mut package: Option<Package> = None;
        if let Some(name) = name {
            if Package::has_name(tbl) {
                match Package::extract_name(tbl) {
                    // package_name = { [1] = "<name>" | name = "<name>" }
                    Ok(package_name) => {
                        warn!(
                            "key Named '{}' overrides package name '{}'",
                            name, package_name
                        );
                    }
                    Err(err) => {
                        warn!("{}", err);
                    }
                }
            }
            package = Some(Package::new(name));
        } else {
            match Package::extract_name(tbl) {
                Ok(name) => {
                    package = Some(Package::new(name));
                }
                Err(err) => {
                    fatal!("{}", err);
                }
            }
        }
        if let Some(mut pkg) = package {
            for pair in tbl.pairs::<Value, Value>() {
                let (k, value): (Value, Value) = pair.unwrap();

                if let Value::String(lua_key) = k {
                    let key: &str = &lua_str_to_str(&lua_key);
                    match key {
                        "links" => {
                            if let Some(tbl) = value.as_table() {
                                pkg.links = Package::extract_links(&tbl);
                            } else {
                                fatal!("expected 'Table', found '{:?}'", value);
                            }
                        }
                        "name" => (),
                        "excludes" => {
                            pkg.excludes = Package::extract_targets(&value);
                        }
                        "templates" => {
                            pkg.templates = Package::extract_targets(&value);
                        }
                        _ => warn!("key '{}' is ignored", key),
                    }
                }
            }
            return pkg;
        }
        unreachable!();
    }

    fn from_pair(pair: (&Value, &Value)) -> Option<Package> {
        match pair {
            (Value::Integer(_), Value::String(name)) => {
                return Some(Package::new(lua_str_to_str(name)));
            }
            (Value::Integer(_), Value::Table(tbl)) => {
                return Some(Package::from_table(None, tbl));
            }
            (Value::String(name), Value::Table(tbl)) => {
                return Some(Package::from_table(Some(lua_str_to_str(name)), tbl));
            }
            (key, value) => {
                fatal!("Unsupported package format: {:?} = {:?}", key, value);
            }
        }
    }
}

struct Context {
    lua: Lua,
    config_path: PathBuf,
}

impl Context {
    fn new() -> Self {
        let app_name = env::var("MDOT_APPNAME").unwrap_or(APP_NAME.to_string());
        let config_dir = dirs::config_dir().unwrap();
        let mut config_path = PathBuf::from(config_dir);
        config_path.push(app_name);
        Self {
            lua: Lua::new(),
            config_path: config_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn test_package_string() {
        let _ = setup_logger();
        let ctx = Context::new();
        let s = ctx.lua.create_string("foo").unwrap();
        let e = Package::new("foo".to_string());
        assert_eq!(
            Package::from_pair((&Value::Integer(1), &Value::String(s))),
            Some(e)
        );
    }

    #[test]
    fn test_package_table() {
        let _ = setup_logger();
        let ctx = Context::new();
        let name_foo = "foo".into_lua(&ctx.lua).unwrap();
        let name_bar = "bar".into_lua(&ctx.lua).unwrap();
        let name_name = "name".into_lua(&ctx.lua).unwrap();
        let expected = Some(Package::new("foo".to_string()));

        let tbl = ctx.lua.create_table().unwrap();
        tbl.set(1, &name_foo).unwrap();

        assert_eq!(
            Package::from_pair((&Value::Integer(1), &Value::Table(tbl.clone()))),
            expected
        );

        tbl.set(1, &name_bar).unwrap();
        assert_eq!(
            Package::from_pair((&name_foo, &Value::Table(tbl.clone()))),
            expected
        );

        tbl.set(1, &name_bar).unwrap();
        tbl.set(name_name.clone(), &name_bar).unwrap();
        assert_eq!(
            Package::from_pair((&name_foo, &Value::Table(tbl.clone()))),
            expected
        );
        tbl.set(name_name.clone(), Value::Nil).unwrap();
        assert_eq!(
            Package::from_pair((&name_foo, &Value::Table(tbl.clone()))),
            expected
        );
        tbl.set(1, Value::Nil).unwrap();
        assert_eq!(
            Package::from_pair((&name_foo, &Value::Table(tbl.clone()))),
            expected
        );
    }
}

fn setup_logger() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            // 2. Define the color based on the level
            let level_color = match record.level() {
                log::Level::Error => record.level().to_string().red(),
                log::Level::Warn => record.level().to_string().yellow(),
                log::Level::Info => record.level().to_string().green(),
                log::Level::Debug => record.level().to_string().blue(),
                log::Level::Trace => record.level().to_string().magenta(),
            };

            out.finish(format_args!(
                "[{}] {}",
                level_color, // 3. Use the colored level
                message
            ))
        })
        .level(log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .apply()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logger()?;
    let ctx = Context::new();
    let conf = ctx.lua.load(
        r#"
  return {
    "ly",
    "fish",
    hypr = {
        depends = {
          "fish",
          "neovim",
          "uwsm"
        },
        pkg = {
          arch = "hyprland",
        },
        exclude = "*",
    },
    git = {
        depends = {
          "hypr",
        },
        excludes = { "as_table", "second" },
        templates = { "as_templates", "second" },
    },
    {
        name = "alacritty",
        links = {
            {
                source = "src",
                targets = { "tar", "hello" },
            },
            ["key-src"] = "value-tar"
        },
        excludes = "as_string",
        templates = "as_templates",
    },
    {
        links = {
            {
                source = "src",
                targets = "tar",
                overwrite = false,
                backup = true,
            },
        },
        "tmux",
    }
  }
  "#,
    );
    let res = conf.eval::<Table>().unwrap();
    for pair in res.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let pkg = Package::from_pair((&key, &value));
        info!("{:#?}", pkg);
        // info!("key: {:?}, value: {:?}", key, value);
    }
    Ok(())
}
