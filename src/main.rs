use colored::*;
use dirs;
use log::{error, info, warn};
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

type OSPackage = HashMap<String, String>;
#[derive(Debug)]
enum OSPackageName {
    AsPackage(bool),
    Name(String),
    Package(OSPackage),
}

#[derive(Debug)]
enum Enabled {
    Enable(bool),
    Hook(Function),
}

#[derive(Debug)]
struct LinkObject {
    source: PathBuf,
    target: PathBuf,
    overwrite: bool,
    backup: bool,
}

#[derive(Default, Debug)]
struct Package {
    name: Option<String>,
    package_name: Option<OSPackageName>,
    // enabled: bool,
    enabled: Option<Enabled>,
    depends: Option<Vec<Package>>,
    links: Option<Vec<LinkObject>>,
    excludes: Option<Vec<PathBuf>>,
    templates: Option<Vec<PathBuf>>,
}

impl Package {
    fn new(name: String) -> Self {
        Self {
            name: Some(name),
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
        if let Some(pkg) = package {
            for pair in tbl.pairs::<Value, Value>() {
                let (k, value) = pair.unwrap();

                if let Value::String(lua_key) = k {
                    let key: &str = &lua_key.to_str().unwrap().to_string();
                    match key {
                        "name" => (),
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
                return Some(Package::new(name.to_str().ok()?.to_string()));
            }
            (Value::Integer(_), Value::Table(tbl)) => {
                return Some(Package::from_table(None, tbl));
            }
            (Value::String(name), Value::Table(tbl)) => {
                return Some(Package::from_table(
                    Some(name.to_str().unwrap().to_string()),
                    tbl,
                ));
            }
            (key, value) => {
                fatal!("Unsupported package format: {:?} = {:?}", key, value);
            }
        }
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
        .chain(fern::log_file("output.log")?)
        .apply()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logger()?;
    let app_name = env::var("MDOT_APPNAME").unwrap_or(APP_NAME.to_string());
    let config_dir = dirs::config_dir().unwrap();

    let mut config_path = PathBuf::from(config_dir);
    config_path.push(app_name);

    let lua = Lua::new();
    let conf = lua.load(
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
    },
    {
        name = "alacritty",
    },
    {
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
