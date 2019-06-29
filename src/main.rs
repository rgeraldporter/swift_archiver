extern crate glob;
extern crate toml;
extern crate curl;
use std::fmt;
use std::fs::{self, DirEntry, File};
use std::path::Path;
use std::io;
use std::io::Read;
use std::io::prelude::*;
use std::env;
use std::process;
use std::error::Error;
use std::collections::HashMap;
use std::io::BufReader;
use glob::glob;
use serde_derive::Deserialize;
use toml::value::Array;
use toml::map::Map;
use toml::Value;
use curl::easy::{Easy, List};

#[derive(Deserialize)]
struct Device {
    name: String,
}

#[derive(Deserialize)]
struct ArchiveOrg {
    access_key: String,
    secret_key: String,
    creator: String,
    subject_tags: toml::value::Array,
    collection_id: String,
    license_url: String,
    base_description: String,
}

#[derive(Deserialize)]
struct Config {
    device: Device,
    archive_org: ArchiveOrg,
}

#[derive(Deserialize)]
struct Site {
    name: String,
    subject_tags: toml::value::Array,
    description: String,
    ready_to_upload: bool,
}

#[derive(Deserialize)]
struct IAManifest {
    identifier: String,
    update: bool,
    files: toml::value::Array,
}

const SWIFT_CONFIG_FILENAME: &str = "swift.toml";
const SWIFT_DEFAULT_CONFIG: &str = r#"[device]
name = "SWIFT"

[archive_org]
access_key = "YOUR_ACCESS_KEY_HERE"
secret_key = "YOUR_SECRET_KEY_HERE"
creator = "Your name here"
subject_tags = ["soundscapes", "Cornell Swift Recorder"]
collection_id = "media"
license_url = "https://creativecommons.org/licenses/by/4.0/"
base_description = """
Recordings from a Cornell Swift Bioacoustics Recorder."""
test_item = true"#;

fn file_to_string(file: File) -> String {

    let mut contents = String::new();
    let mut file = &file;
    file.read_to_string(&mut contents)
        .expect("something went wrong reading the file");

    contents
}

fn read_file(file_path: &str) -> String {

    match File::open(file_path) {
        Ok(file) => file_to_string(file),
        Err(_) => "".to_string(),
    }
}

fn prompt_yn(text: &str) -> String {

    let mut yn = String::new();
    println!("{}", text);
    io::stdout().flush();
    io::stdin().read_line(&mut yn).expect("Failed to read line");

    yn
}

// specific to swift file right now, should be made more generic
fn read_file_or(file_path: &str, default: &str) -> String {

    let mut contents = match File::open(file_path) {
        Ok(file) => file_to_string(file),
        Err(_) => "".to_string(),
    };

    if contents.len() == 0 {

        let prompt = format!("No `{}` file was found, do you want one created with placeholder values?", file_path);

        loop {
            match prompt_yn(&prompt).trim() {
                "y"|"Y" => break,
                "n"|"N" => {
                    println!("You will need a `{}` file in the root directory of your recordings to use this tool.", file_path);
                    process::exit(1)
                },
                _ => continue,
            };
        }

        contents = default.to_string();
        save_swift_config_from_string(&contents);
    }

    contents
}

fn load_swift_config(swift_file_path: &str) -> Config {

    let toml_str: String = read_file_or(swift_file_path, SWIFT_DEFAULT_CONFIG).to_owned();
    let toml_str_slice: &str = &toml_str[..];
    toml::from_str(toml_str_slice).unwrap()
}

fn load_site(file_path: &str) -> Site {

    let toml_str: String = read_file(file_path).to_owned();
    let toml_str_slice: &str = &toml_str[..];
    toml::from_str(toml_str_slice).unwrap()
}

fn load_manifest(file_path: &str) -> IAManifest {

    let toml_str: String = read_file(file_path).to_owned();
    let toml_str_slice: &str = &toml_str[..];
    toml::from_str(toml_str_slice).unwrap()
}

fn save_swift_config_from_string(config: &str) {

    let mut file = std::fs::File::create(SWIFT_CONFIG_FILENAME).unwrap();
    file.write_all(config.as_bytes()).expect("Could not write new swift.toml file!");
}

fn save_manifest(v: IAManifest) {

    let mut manifest = toml::map::Map::new();
    manifest.insert("identifier".into(), Value::String(v.identifier));
    manifest.insert("update".into(), Value::Boolean(v.update));
    manifest.insert("files".into(), Value::Array(v.files));

    let mut map = toml::map::Map::new();
    map.insert("manifest".into(), Value::Table(manifest));

    let manifest = toml::ser::to_string_pretty(&Value::Table(map)).unwrap();

    let mut file = std::fs::File::create("ia_manifest.toml").unwrap();
    file.write_all(manifest.as_bytes()).expect("Could not write manifest to file!");
}

fn directory_list(file_path: &str) -> Result<Vec<std::path::PathBuf>, Box<Error>> {

    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    for entry in fs::read_dir(file_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            dirs.push(path);
        }
    }

    Ok(dirs)
}

fn upload_file(bucket: &str, file_path: &str, config: Config) {

    let mut file = File::open(file_path).unwrap();
    let ia_url = format!("https://s3.us.archive.org/{}/{}", bucket, file_path);
    let mut headers = List::new();
    let auth_header = format!("authorization:LOW {}:{}", config.archive_org.access_key, config.archive_org.secret_key);

    headers.append(&auth_header).unwrap();
    headers.append("x-amz-auto-make-bucket:1").unwrap();
    headers.append("x-archive-meta01-collection:test_collection").unwrap();
    headers.append("x-archive-meta-mediatype:audio").unwrap();

    let mut handle = Easy::new();

    handle.url(&ia_url).unwrap();
    handle.http_headers(headers).unwrap();
    handle.put(true).unwrap();
    handle.post_field_size(file.metadata().unwrap().len() as u64).unwrap();

    let mut transfer = handle.transfer();

    // NOTE: CHECK IF API KEYS WERE IN CODE IN LAST COMMIT!
    transfer.read_function(|buf| {
        Ok(file.read(buf).unwrap_or(0))
    }).unwrap();

    // right now only does something if there's an Error
    transfer.write_function(|data| {
        println!("{}", std::str::from_utf8(data).unwrap());
        Ok(data.len())
    }).unwrap();

    transfer.perform().unwrap()
}

fn main() {

    // commands
    // swift upload - upload any that have not completed yet
    // swift prepare - prepare directories for upload (site.toml)
    // swift status - notify which dirs are unprepared (not ready_to_upload or missing files)
    // swift test - verify API key and connection is good by uploading a test item

    // 1. read a config file
    let config: Config = load_swift_config(SWIFT_CONFIG_FILENAME);

    // 2. check each first-level directory for site.toml; add them into any that are missing
    let site_configs = glob("./*/site.toml").expect("Failed to read glob pattern");

    for dir in directory_list("./").unwrap() {
        let site_path: String = format!("{}/site.toml", dir.to_str().unwrap());
        let site_config = Path::new(&site_path).exists();

        if site_config {
            println!("found site config in: {}", site_path);
        } else {
            println!("MISSING: {}", site_path);
            // @todo add the file, or prompt to add the file?
        }
    }
/*
    for file in site_configs {
        let file = file.unwrap();
        let file = file.display();

        let site: Site = load_site(&file.to_string());

        println!("{:?}", file);
    }
*/

    // 3. check each second-level directory for an ia_manifest.toml
    let manifest_configs =  glob("./*/*/ia_manifest.toml").expect("Failed to read glob pattern");

    for file in manifest_configs {
        let file = file.unwrap();
        let file = file.display();

        load_manifest(&file.to_string());
    }

    // 4. if no ia_manifest.toml, create one
    // 5. only upload one dir (day) at a time (for now), one file at a time
    // 6. log each file when completed to `files`, skip uploading files already listed there (this way can be resumed)
    // 7. set `update` to false when complete

    let mut wav_files = toml::value::Array::new();
    wav_files.insert(0, Value::String("HNCSW2_20190615_004000.wav".to_string()));

    let ia_manifest = IAManifest {
        identifier: "HNCSW220190615".to_string(),
        update: false,
        files: wav_files,
    };

    save_manifest(ia_manifest);

    // example usage
    //upload_file("HNC-RUST-TEST", "./Waterloo10.m4a", config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_can_read_config() {

        let config: Config = load_swift_config("src/assets/swift.toml");

        assert_eq!(config.device.name, "SWIFT");
        assert_eq!(config.archive_org.creator, "Your name here");
        assert_eq!(config.archive_org.access_key, "YOUR_ACCESS_KEY_HERE");
        assert_eq!(config.archive_org.secret_key, "YOUR_SECRET_KEY_HERE");
    }

    #[test]
    fn it_can_read_site() {

        let mut site: Site = load_site("src/assets/site.toml");

        let mut subject_tags = toml::value::Array::new();
        subject_tags.insert(0, Value::String("Dundas, Ontario".to_string()));
        subject_tags.insert(0, Value::String("Cartwright Nature Sanctuary".to_string()));

        assert_eq!(site.subject_tags, subject_tags);
    }

    #[test]
    fn it_can_read_ia_manifest() {

        let ia_manifest: IAManifest = load_manifest("src/assets/ia_manifest.toml");

        assert_eq!(ia_manifest.identifier, "HNCSW220190615");
    }
}