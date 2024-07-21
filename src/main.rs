use actix_files as fs;
use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use chrono::{Duration, Utc};
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use rss::extension::itunes::{ITunesChannelExtensionBuilder, NAMESPACE};
use rss::{ChannelBuilder, EnclosureBuilder, Item, ItemBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Result;
use std::collections::BTreeMap;
use std::env;
use std::fs::read_dir;
use std::fs::read_to_string;
use std::path::Path;

use r2d2_sqlite::SqliteConnectionManager;

type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Deserialize)]
struct LibationSettings {
    Books: String,
}

#[derive(Deserialize)]
struct BooksMeta {
    Books: String,
}

#[derive(Debug)]
struct BookData {
    title: String,
    description: String,
}

struct AppState {
    libation_folder: Box<Path>,
    books_folder: Box<Path>,
    base_url: Box<String>,
    db_pool: Pool,
}

fn generate_feed(
    title: &str,
    book_id: &str,
    book_folder_name: &str,
    base_url: &str,
    image_path: &str,
    audio_paths: &Vec<String>,
) -> Option<String> {
    let namespaces: BTreeMap<String, String> = [("itunes".to_string(), NAMESPACE.to_string())]
        .iter()
        .cloned()
        .collect();
    let itunes_extension = ITunesChannelExtensionBuilder::default()
        .image(format!(
            "{}/libation-files/{}/{}",
            base_url, book_folder_name, image_path
        ))
        .block("Yes".to_string())
        .build();
    let mut items: Vec<Item> = Default::default();
    let today = Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    for (i, file) in audio_paths.iter().enumerate() {
        let pub_date = (today - Duration::days(i as i64)).to_rfc2822();
        let enclosure = EnclosureBuilder::default()
            .url(format!(
                "{}/libation-files/{}/{}",
                base_url, book_folder_name, file
            ))
            .mime_type(String::from("audio/mpeg"))
            .length(file.len().to_string())
            .build();
        let item = ItemBuilder::default()
            .title(Some(file.replace('_', " ").to_owned()))
            .enclosure(Some(enclosure))
            .pub_date(pub_date)
            .build();
        items.push(item);
    }
    let channel = ChannelBuilder::default()
        .namespaces(namespaces)
        .title(title)
        .itunes_ext(itunes_extension)
        .items(items)
        .build();
    Some(channel.to_string())
}

#[get("/libation-feed/{book_id}.rss")]
async fn book_feed(app_state: web::Data<AppState>, book_id: web::Path<String>) -> impl Responder {
    let folder_tag = format!("[{}]", book_id);
    let mut paths = read_dir(app_state.books_folder.clone()).unwrap();

    let pool = app_state.db_pool.clone();
    let  row = pool.get().unwrap().query_row("SELECT Title, Description FROM Books WHERE AudibleProductId = (?)", &[&book_id.to_string()],
        |row| Ok(BookData {
            title: row.get(0)?,
            description: row.get(1)?,
        }));

    let found = paths.find(|path| {
        if let Ok(ref dir_entry) = path {
            if let Ok(file_type) = dir_entry.file_type() {
                if file_type.is_dir() {
                    if let Some(file_name) = dir_entry.file_name().to_str() {
                        if file_name.contains(&folder_tag) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    });
    if let Some(Ok(dir_entry)) = found {
        let mut meta_path: Option<String> = None;
        let mut image_path: Option<String> = None;
        let mut audio_paths: Vec<String> = Vec::new();
        println!("{:?}", dir_entry);
        let files = read_dir(dir_entry.path()).unwrap();
        for file in files {
            if let Ok(dir_entry) = file {
                if let Ok(file_type) = dir_entry.file_type() {
                    if !file_type.is_file() {
                        continue;
                    }
                    if let Some(file_name) = dir_entry.file_name().to_str() {
                        if file_name.ends_with(".json") {
                            meta_path = Some(String::from(file_name));
                        } else if file_name.ends_with(".jpg") {
                            image_path = Some(String::from(file_name));
                        } else if file_name.ends_with(".mp3") {
                            audio_paths.push(String::from(file_name));
                        }
                    }
                }
            }
        }
        audio_paths.sort();
        println!("{:?}", meta_path);
        println!("{:?}", image_path);
        println!("{:?}", audio_paths);
        let title = if let Ok(row_data) = row { row_data.title } else { String::from("")};
        return generate_feed(
            &title,
            &book_id,
            dir_entry.file_name().to_str().unwrap(),
            &app_state.base_url,
            image_path.unwrap().as_ref(),
            &audio_paths,
        );
    }
    None
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let libation_folder = env::var("LIBATION_FOLDER").unwrap();
    let base_url = env::var("FEEDSERVER_BASE_URL").unwrap();
    let port = env::var("FEEDSERVER_PORT").unwrap_or(String::from("8677")).parse::<u16>().unwrap();
    println!("Libation folder: {}", libation_folder);
    let libation_folder = Path::new(&libation_folder);
    let libation_settings: LibationSettings = serde_json::from_str(
        read_to_string(libation_folder.join("Settings.json"))
            .expect("Should have been able to read the file")
            .as_ref(),
    )?;

    // connect to SQLite DB
    let manager = SqliteConnectionManager::file(libation_folder.join("LibationContext.db"));
    let pool = Pool::new(manager).unwrap();

    let app_state = web::Data::new(AppState {
        libation_folder: libation_folder.into(),
        books_folder: Path::new(&libation_settings.Books).into(),
        base_url: base_url.into(),
        db_pool: pool,
    });

    HttpServer::new(move || {
        App::new()
            .app_data(app_state.clone())
            .service(
                fs::Files::new("/libation-files", &libation_settings.Books).show_files_listing(),
            )
            .service(book_feed)
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
