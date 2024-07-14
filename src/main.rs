use actix_files as fs;
use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use serde_json::Result;
use std::fs::read_to_string;
use std::fs::read_dir;
use std::path::Path;
use std::env;
use r2d2_sqlite::SqliteConnectionManager;

type Pool = r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>;
type Connection = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

#[derive(Deserialize)]
struct LibationSettings {
    Books: String,
}

#[derive(Deserialize)]
struct BookMeta {
    Books: String,
}

struct AppState {
    libation_folder: Box<Path>,
    books_folder: Box<Path>,
    base_url: Box<String>,
    db_pool: Pool,
}

#[get("/book-feed/{book_id}.rss")]
async fn book_feed(
    app_state: web::Data<AppState>,
    book_id: web::Path<String>,
) -> impl Responder {
    let folder_tag = format!("[{}]", book_id);
    let mut paths = read_dir(app_state.books_folder.clone()).unwrap();

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
                        }
                        else if file_name.ends_with(".mp3") {
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
    }
    format!("Book id: {}", book_id)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let libation_folder = env::var("LIBATION_FOLDER").unwrap();
    let base_url = env::var("BASE_URL").unwrap();
    println!("Libation folder: {}", libation_folder);
    let libation_folder = Path::new(&libation_folder);
    let libation_settings: LibationSettings = serde_json::from_str(
        read_to_string(libation_folder.join("Settings.json"))
        .expect("Should have been able to read the file")
        .as_ref())?;

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
            .service(fs::Files::new("/files",  &libation_settings.Books).show_files_listing())
            .service(book_feed)
    })
    .bind(("0.0.0.0", 8677))?
    .run()
    .await
}
