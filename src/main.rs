use actix_files::NamedFile;
use actix_multipart::Multipart;
use actix_web::{get, post, web, App, HttpServer, HttpResponse, Result}; 
use futures_util::stream::TryStreamExt;
use image::{ImageFormat, io::Reader as ImageReader};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use serde::Deserialize;
use std::env;
use actix_cors::Cors;


struct AppState {
    task_id_counter: Mutex<i32>,
}

#[derive(Deserialize)]
struct ConvertParams {
    output_format: String,
}

#[post("/convert")]
async fn convert_image_endpoint(
    state: web::Data<AppState>,
    mut payload: Multipart,
    query: web::Query<ConvertParams>,
) -> Result<HttpResponse> {
    let output_format = &query.output_format;
    let mut file_path: Option<PathBuf> = None;
    let mut original_filename = String::new();

    while let Some(mut field) = payload.try_next().await? {  // Declare field as mutable
        if let Some(filename) = field.content_disposition().get_filename() {
            original_filename = filename.to_string();
            let filepath = Path::new("uploads").join(&filename);
            let mut f = fs::File::create(filepath.clone()).await?;
            
            while let Some(chunk) = field.try_next().await? {  // This will now work
                f.write_all(&chunk).await?;
            }
            f.sync_all().await?;
            file_path = Some(filepath);
        } else {
            return Ok(HttpResponse::BadRequest().body("No filename provided in the request"));
        }
    }

    if let Some(input_file_path) = file_path {
        let mut task_id_counter = state.task_id_counter.lock().unwrap();
        let task_id = *task_id_counter + 1;
        *task_id_counter = task_id;

        let output_file_path = generate_output_filepath(&original_filename, output_format, task_id);

        match convert_image(&input_file_path, output_format, &output_file_path).await {
            Ok(_) => Ok(HttpResponse::Ok().json({
                serde_json::json!({
                    "task_id": task_id,
                    "converted_file": format!("/download/{}", output_file_path.file_name().unwrap().to_str().unwrap()),
                })
            })),
            Err(e) => Ok(HttpResponse::InternalServerError().body(format!("Conversion failed: {}", e))),
        }
    } else {
        Ok(HttpResponse::BadRequest().body("No file uploaded"))
    }
}


fn generate_output_filepath(filename: &str, output_format: &str, task_id: i32) -> PathBuf {
    let stem = Path::new(filename).file_stem().unwrap().to_str().unwrap();
    Path::new("downloads").join(format!("{}_{}.{}", stem, task_id, output_format))
}

// Serve the converted image files
#[get("/download/{filename}")]
async fn serve_converted_image(filename: web::Path<String>) -> Result<NamedFile> {
    let path = Path::new("downloads").join(filename.into_inner());
    
    println!("Serving file from path: {:?}", path);

    if path.exists() {
        Ok(NamedFile::open(path)?)
    } else {
        println!("File not found: {:?}", path);
        Err(actix_web::error::ErrorNotFound("File not found"))
    }
}

async fn convert_image(input_file: &Path, output_format: &str, output_file: &Path) -> Result<(), String> {
    println!("Converting image from {:?} to {:?}", input_file, output_format);

    let mut img = match ImageReader::open(input_file) {
        Ok(reader) => match reader.decode() {
            Ok(decoded_img) => decoded_img,
            Err(e) => {
                println!("Failed to decode image: {:?}", e);
                return Err(format!("Failed to decode image: {:?}", e));
            }
        },
        Err(e) => {
            println!("Failed to open input image: {:?}", e);
            return Err(format!("Failed to open input image: {:?}", e));
        }
    };

    if output_format == "ico" {
        img = img.thumbnail(256, 256);
    }

    let result = match output_format {
        "png" => img.save_with_format(output_file, ImageFormat::Png),
        "jpg" => img.save_with_format(output_file, ImageFormat::Jpeg),
        "gif" => img.save_with_format(output_file, ImageFormat::Gif),
        "bmp" => img.save_with_format(output_file, ImageFormat::Bmp),
        "webp" => img.save_with_format(output_file, ImageFormat::WebP),
        "ico" => img.save_with_format(output_file, ImageFormat::Ico),
        "tiff" => img.save_with_format(output_file, ImageFormat::Tiff),
        _ => return Err("Unsupported output format".to_string()),
    };

    if let Err(e) = result {
        println!("Failed to save output image: {:?}", e);
        return Err(format!("Image conversion failed: {:?}", e));
    }

    match ImageReader::open(output_file) {
        Ok(_) => println!("Successfully validated saved image: {:?}", output_file),
        Err(e) => {
            println!("Failed to reopen and validate output image: {:?}", e);
            return Err(format!("Failed to reopen output image: {:?}", e));
        }
    }

    Ok(())
}



#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env::set_current_dir(env::current_dir().unwrap()).unwrap();
    fs::create_dir_all("uploads").await?;
    fs::create_dir_all("downloads").await?;

    // Define the application state
    let state = web::Data::new(AppState {
        task_id_counter: Mutex::new(0),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone()) // This passes the state to the application
            .wrap(Cors::permissive()) // Use permissive CORS for development
            .service(convert_image_endpoint)
            .service(serve_converted_image)
    })
    .bind("0.0.0.0:8000")?  // Bind to all available IP addresses
    .run()
    .await
}
