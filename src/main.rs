extern crate glob;
extern crate toml;
use std::fs::File;
use std::io::Read;
use std::io::prelude::*;
use glob::glob;
use serde_derive::Deserialize;
use toml::value::Array;
use toml::map::Map;
use toml::Value;

#[derive(Deserialize)]
struct Device {
    name: String,
}

#[derive(Deserialize)]
struct ArchiveOrg {
    api_key: String,
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
}

#[derive(Deserialize)]
struct IAManifest {
    identifier: String,
    update: bool,
    files: toml::value::Array,
}

fn read_file(file_path: &str) -> String {

    // @todo handle better, need to return a good error.
    let mut f = File::open(file_path).expect(&format!("{} not found", file_path));

    let mut contents = String::new();
    f.read_to_string(&mut contents)
        .expect("something went wrong reading the file");

    contents
}

fn populate_config() {

    let toml_str: String = read_file("swift.toml").to_owned();
    let toml_str_slice: &str = &toml_str[..];
    let config: Config = toml::from_str(toml_str_slice).unwrap();

    println!("{}\n{}\n{}", config.device.name, config.archive_org.creator, config.archive_org.api_key);

    for x in &config.archive_org.subject_tags {
        println!("general subject tag: {}", x);
    }
}

fn find_sw_files(pattern: &str) -> glob::Paths {

    glob(pattern).expect("Failed to read glob pattern")
}

fn load_site_config(file_path: &str) {
    let toml_str: String = read_file(file_path).to_owned();
    let toml_str_slice: &str = &toml_str[..];
    let site_config: Site = toml::from_str(toml_str_slice).unwrap();

    for x in &site_config.subject_tags {
        println!("site subject tag: {}", x);
    }
}

fn load_ia_manifest(file_path: &str) {
    let toml_str: String = read_file(file_path).to_owned();
    let toml_str_slice: &str = &toml_str[..];
    let ia_manifest: IAManifest = toml::from_str(toml_str_slice).unwrap();

    println!("identifier: {}", ia_manifest.identifier);
}

fn to_toml(v: Vec<(String, (String, u32))>) -> Value {
    let mut servers = toml::map::Map::new();
    for (name, (ip_addr, port)) in v {
        let mut server = toml::map::Map::new();
        server.insert("Ipaddr".into(), Value::String(ip_addr));
        server.insert("Port no".into(), Value::Integer(port as i64));
        servers.insert(name, Value::Table(server));
    }

    let mut map = toml::map::Map::new();
    map.insert("server".into(), Value::Table(servers));
    Value::Table(map)
}

fn main() {

    // 1. read a config file
    populate_config();

    // 2. check each directory for site.toml
    let files = find_sw_files("./*/site.toml");

    for file in files {
        let file = file.unwrap();
        let file = file.display();
        //println!("{}", file);

        load_site_config(&file.to_string());
    }

    // 3. check those directories for an ia_manifest.toml
    let files = find_sw_files("./*/*/ia_manifest.toml");

    for file in files {
        let file = file.unwrap();
        let file = file.display();
        //println!("{}", file);

        load_ia_manifest(&file.to_string());
    }

    // 4. if no ia_manifest.toml, create one
    // 5. only upload one dir (day) at a time (for now), one file at a time
    // 6. log each file when completed to `files`, skip uploading files already listed there (this way can be resumed)
    // 7. set `update` to false when complete

    // test create a toml file

    let v = vec![("A".into(), ("192.168.4.1".into(), 4476)),
                 ("B".into(), ("192.168.4.8".into(), 1234))];

    let toml_string = toml::ser::to_string_pretty(&to_toml(v)).unwrap();
    println!("{}", toml_string);

    let mut file = std::fs::File::create("servers.toml").unwrap();
    file.write_all(toml_string.as_bytes()).expect("Could not write to file!");
}