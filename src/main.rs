extern crate toml;
extern crate curl;
extern crate colored;
use colored::Colorize;
use std::fs::{self, File};
use std::path::Path;
use std::io;
use std::env;
use std::io::Read;
use std::io::prelude::*;
use std::process;
use std::error::Error;
use std::time::Instant;
use serde_derive::Deserialize;
use toml::value::*;
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
    test_mode: bool
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

const ARCHIVE_ORG_META_PREFIX: &str = "swift-archiver--";
const SWIFT_ARCHIVER_AGENT_NAME: &str = "Swift Archiver v0.1.1";
const SWIFT_ARCHIVER_AGENT_URL: &str = "https://github.com/rgeraldporter/swift_archiver";
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
test_mode = true"#;

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

fn prompt_query(text: &str) -> String {

    let mut answer = String::new();
    println!("{}", text);
    io::stdout().flush();
    io::stdin().read_line(&mut answer).expect("Failed to read line");

    answer.trim().to_string()
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
            match prompt_query(&prompt).trim() {
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

fn save_site(s: Site, path: &str) {

    let mut site = toml::map::Map::new();
    site.insert("name".into(), Value::String(s.name));
    site.insert("subject_tags".into(), Value::Array(s.subject_tags));
    site.insert("description".into(), Value::String(s.description));
    site.insert("ready_to_upload".into(), Value::Boolean(s.ready_to_upload));

    let site = toml::ser::to_string_pretty(&Value::Table(site)).unwrap();
    let site_path: String = format!("{}/site.toml", path);
    let mut file = std::fs::File::create(site_path).unwrap();

    file.write_all(site.as_bytes()).expect("Could not write site.toml file!");
}

fn save_manifest(v: IAManifest, path: &str) {

    let mut manifest = toml::map::Map::new();
    manifest.insert("identifier".into(), Value::String(v.identifier));
    manifest.insert("update".into(), Value::Boolean(v.update));
    manifest.insert("files".into(), Value::Array(v.files));

    let manifest = toml::ser::to_string_pretty(&Value::Table(manifest)).unwrap();

    let mut file = std::fs::File::create(path).unwrap();
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

fn file_list(file_path: &str) -> Result<Vec<std::path::PathBuf>, Box<Error>> {

    let mut files: Vec<std::path::PathBuf> = Vec::new();

    for entry in fs::read_dir(file_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            files.push(path);
        }
    }

    Ok(files)
}

// for standard IA meta headers
fn ia_meta_header(key: &str, value: &str) -> String {
    let ia_header_prefix: &str = "x-archive-meta-";
    [ia_header_prefix, key, ":", value].join("")
}

// for custom meta headers
fn ia_swift_meta_header(key: &str, value: &str) -> String {
    let ia_header_prefix: &str = "x-archive-meta-";
    [ia_header_prefix, ARCHIVE_ORG_META_PREFIX, key, ":", value].join("")
}

fn upload_file(bucket: &str, file_path: &str, config: &Config, site: &Site, date: &str) {

    let mut file = File::open(file_path).unwrap();
    let file_name = Path::new(&file_path).file_name().unwrap().to_str().unwrap();
    let ia_url = format!("https://s3.us.archive.org/{}/{}", bucket, file_name);
    let mut headers = List::new();
    let auth_header = format!("authorization:LOW {}:{}", config.archive_org.access_key, config.archive_org.secret_key);
    let date_for_title = str::replace(&date, "-", ".");
    let title = format!("{} {} {} Soundscape", &config.device.name, date_for_title, &site.name);
    let mut subject_tags_text = String::new();
    let mut description = String::new();

    description.push_str(&config.archive_org.base_description);
    description.push_str(&site.description);

    let collection = match &config.archive_org.test_mode {
        true => "test_collection",
        false => &config.archive_org.collection_id
    };
    let mut collection_meta = String::new();

    collection_meta.push_str("x-archive-meta01-collection:");
    collection_meta.push_str(collection);

    // base subject tags
    for tag in &config.archive_org.subject_tags {
        subject_tags_text.push_str(tag.as_str().unwrap());
        subject_tags_text.push_str(";");
    }

    // site-specific subject tags
    for tag in &site.subject_tags {
        subject_tags_text.push_str(tag.as_str().unwrap());
        subject_tags_text.push_str(";");
    }

    headers.append(&auth_header).unwrap();
    headers.append("x-amz-auto-make-bucket:1").unwrap();
    headers.append(&collection_meta).unwrap();
    headers.append(&ia_meta_header("creator", &config.archive_org.creator)).unwrap();
    headers.append(&ia_meta_header("date", &date)).unwrap();
    headers.append(&ia_meta_header("licenseurl", &config.archive_org.license_url)).unwrap();
    headers.append(&ia_meta_header("subject", subject_tags_text.trim_end_matches(';'))).unwrap();
    headers.append(&ia_meta_header("mediatype", "audio")).unwrap();
    headers.append(&ia_meta_header("title", &title)).unwrap();
    headers.append(&ia_meta_header("description", &description)).unwrap();
    headers.append(&ia_meta_header("scanner", SWIFT_ARCHIVER_AGENT_NAME)).unwrap();
    headers.append(&ia_swift_meta_header("url", SWIFT_ARCHIVER_AGENT_URL)).unwrap();
    headers.append(&ia_swift_meta_header("deviceprefix", &config.device.name)).unwrap();
    headers.append(&ia_swift_meta_header("location", &site.name)).unwrap();
    //headers.append(&ia_swift_meta_header("gps", "GPS COORDS TO GO HERE")).unwrap();

    // in case we need to look at headers
    // @todo env var for this, verbose mode maybe?
    //println!("{:#?}", headers);

    let mut handle = Easy::new();

    handle.url(&ia_url).unwrap();
    handle.http_headers(headers).unwrap();
    handle.put(true).unwrap();
    handle.post_field_size(file.metadata().unwrap().len() as u64).unwrap();

    let mut transfer = handle.transfer();

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

fn strip_characters(original : &str, to_strip : &str) -> String {
    original.chars().filter(|&c| !to_strip.contains(c)).collect()
}

fn determine_date(folder_path: &str) -> &str {
    folder_path.split('_').nth(1).unwrap_or("")
}

fn determine_identifier(folder_path: &str, device_name: &str) -> String {

    let date_portion = folder_path.split('_').nth(1).unwrap_or("");
    let date_identifier: String = strip_characters(date_portion, "-");

    let mut identifier = String::new();
    identifier.push_str(device_name);
    identifier.push_str(&date_identifier);
    identifier
}

fn site_questionnaire(path: &str) -> Site {
    let site_name = prompt_query("What is the name of the site location?");
    let subject_tags_response = prompt_query("What subject tags should the archives contain? Separate them by semicolon, e.g. 'soundscape; Hamilton, Ontario; wetland'.");
    let description = prompt_query("What description would you like to give these archives?");
    let split = subject_tags_response.trim().split(";");
    let mut subject_tags = toml::value::Array::new();

    for s in split {
        subject_tags.insert(0, toml::Value::String(s.trim().to_string()));
    }

    let site = Site {
        name: site_name,
        subject_tags: subject_tags,
        description: description,
        ready_to_upload: true,
    };

    save_site(site, &path);
    let site_path: String = format!("{}/site.toml", path);
    load_site(site_path.as_str())
}

fn upload_collection(path: &str, config: Config) {

    // check for site data, if not present, prompt for data
    let site_path: String = format!("{}/site.toml", &path);
    let site: Site = match Path::new(&site_path).exists() {
        true => load_site(&site_path),
        false => site_questionnaire(&path)
    };

    for dir in directory_list(path).unwrap() {

        let date = determine_date(dir.to_str().unwrap());
        let identifier = determine_identifier(dir.to_str().unwrap(), &config.device.name);
        let dir_path = format!("{}", dir.to_str().unwrap());
        let manifest_path = format!("{}/manifest.toml", &dir_path);

        println!(
            "\n{}{}{}{}",
            "Uploading: Identifier ".green(),
            &identifier.green(),
            ", from folder ".green(),
            &dir_path.green()
        );

        let now = Instant::now();

        // @todo make fn
        println!("Uploading file: {}", &site_path);
        upload_file(&identifier, &site_path, &config, &site, &date);
        println!("{}{}{}", "Upload complete in ".green(), now.elapsed().as_secs().to_string().green(), " seconds.".green());
        println!("{}{}{}", &identifier.yellow(), " is now available at https://archive.org/details/".yellow(), &identifier.yellow());
        println!("{}", "Uploading recordings...".yellow());

        for file in file_list(&dir_path).unwrap() {

            let file_path = format!("{}", file.to_str().unwrap());
            let file_name = Path::new(&file_path).file_name().unwrap().to_str().unwrap();
            let first_char = file_name.chars().next().unwrap();

            if first_char == '.' || file_name == "manifest.toml" {
                println!("{}{}", "skipping ignored file:".cyan(), &file_name.cyan());
                continue;
            }

            let mut manifest: IAManifest = match Path::new(&manifest_path).exists() {
                true => load_manifest(&manifest_path),
                false => IAManifest {
                    identifier: identifier.to_owned(),
                    update: true,
                    files: toml::value::Array::new()
                }
            };

            let mut iter = manifest.files.iter();

            match iter.find(|&x| x.as_str() == Some(&file_name)) {
                Some(file) => {
                    println!("{}{}", "Skipping already uploaded file:".cyan(), file.as_str().unwrap().cyan());
                    continue;
                },
                None => ()
            };

            let now = Instant::now();

            println!("Uploading file: {}", file.to_str().unwrap());
            upload_file(&identifier, &file_path, &config, &site, &date);
            manifest.files.push(toml::value::Value::String(file_name.to_string()));
            println!("{}{}{}", "Upload complete in ".green(), now.elapsed().as_secs().to_string().green(), " seconds.".green());
            save_manifest(manifest, &manifest_path);
        };

        println!("{}{}{}", &identifier.yellow(), " is now uploaded to archive.org! https://archive.org/details/".yellow(), &identifier.yellow());
    }
}

fn cmd_upload(args: &Vec<String>) {

    let config: Config = load_swift_config(SWIFT_CONFIG_FILENAME);
    let folder = &args[2];

    upload_collection(&folder, config);
}

fn cmd_help() {
    println!("`help` not fully implemented yet.");
    println!("`swarc upload [dir_path]`: Start upload of specified directory path.");
}

fn main() {

    // commands
    // swarc upload [dir] - upload any that have not completed yet
    // swarc prepare - prepare directories for upload (site.toml)
    // swarc status - notify which dirs are unprepared (not ready_to_upload or missing files)
    // swarc test - verify API key and connection is good by uploading a test item

    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    match command.as_str() {
        "upload" => cmd_upload(&args),
        _ => cmd_help()
    }

    // @todo, if no config file, create one!
    // @todo test API_KEY and secret to ensure it's valid
    // @todo handle case where identifier would be identical to existing (moved same date)
    // perhaps do that by checking first file name; if not 000000, append time after dash
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

        let site: Site = load_site("src/assets/site.toml");

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

    #[test]
    fn it_can_create_identifier() {
        let device_name = "SWX";
        let folder_name = "SWX_2019-01-16";
        let identifier = determine_identifier(folder_name, device_name);

        assert_eq!(identifier, "SWX20190116");
    }
}