use reqwest::{Client, header};
use serde_json::Value;
use std::error::Error;
use std::io::{self, Write, BufRead, BufReader};
use std::fs::{self, File};
use std::path::Path;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration};

fn get_cookie() -> Result<String, Box<dyn Error>> {
    let cookie_file = "pixiv_cookie.txt";

    if let Ok(file) = File::open(cookie_file) {
        let reader = BufReader::new(file);
        if let Some(Ok(cookie)) = reader.lines().next() {
            println!("从文件中读取到Cookie");
            return Ok(cookie);
        }
    }

    println!("由于Pixiv的登录过程包含人机验证，我们需要手动获取Cookie。");
    println!("请按照以下步骤操作：");
    println!("1. 打开浏览器，访问 https://www.pixiv.net/ 并登录您的账户");
    println!("2. 登录成功后，按F12打开开发者工具");
    println!("3. 在开发者工具中，切换到 '存储' 标签");
    println!("4. 找到cookie栏目");
    println!("5. 在标签中，找到一个 www.pixiv.net ");
    println!("6. 在名称中找到PHHSESSID");
    println!("7. 复制PHPSESSID的值（前面加上PHPSESSID=）");
    println!();
    print!("请粘贴您复制的Cookie值: ");
    io::stdout().flush()?;

    let mut cookie = String::new();
    io::stdin().read_line(&mut cookie)?;
    let cookie = cookie.trim().to_string();

    // 将cookie保存到文件
    let mut file = File::create(cookie_file)?;
    writeln!(file, "{}", cookie)?;
    println!("Cookie已保存到文件");

    Ok(cookie)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36")
        .build()?;

    let cookie = get_cookie()?;
    println!("Cookie length: {}", cookie.len());

    print!("请输入用户 ID: ");
    io::stdout().flush()?;
    let mut user_id = String::new();
    io::stdin().read_line(&mut user_id)?;
    let user_id = user_id.trim();

    fs::create_dir_all("downloads")?;

    let all_artwork_ids = get_all_artwork_ids(&client, &cookie, user_id).await?;
    println!("Total artworks found: {}", all_artwork_ids.len());

    let semaphore = Arc::new(Semaphore::new(5));
    let client = Arc::new(client);
    let cookie = Arc::new(cookie);

    let results = stream::iter(all_artwork_ids)
        .map(|id| {
            let client = Arc::clone(&client);
            let cookie = Arc::clone(&cookie);
            let semaphore = Arc::clone(&semaphore);
            async move {
                let _permit = semaphore.acquire().await.unwrap();
                let result = download_artwork(&client, &cookie, &id).await;
                if let Err(ref e) = result {
                    eprintln!("Failed to download artwork {}: {}", id, e);
                }
                result
            }
        })
        .buffer_unordered(5)
        .collect::<Vec<_>>()
        .await;

    let successful_downloads = results.iter().filter(|r| r.is_ok()).count();
    println!("Successfully downloaded {} artworks out of {}.", successful_downloads, results.len());

    Ok(())
}

async fn get_all_artwork_ids(client: &Client, cookie: &str, user_id: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let api_url = format!(
        "https://www.pixiv.net/ajax/user/{}/profile/all?lang=zh",
        user_id
    );
    println!("Fetching all artwork IDs: {}", api_url);

    let res = client
        .get(&api_url)
        .header(header::COOKIE, cookie)
        .header(header::REFERER, "https://www.pixiv.net/")
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(format!("Failed to fetch API: {}", res.status()).into());
    }

    let json: Value = res.json().await?;

    let mut artwork_ids = Vec::new();

    for category in &["illusts", "manga"] {
        if let Some(works) = json["body"][category].as_object() {
            artwork_ids.extend(works.keys().cloned());
        }
    }

    Ok(artwork_ids)
}

async fn download_artwork(client: &Client, cookie: &str, artwork_id: &str) -> Result<(), Box<dyn Error>> {
    let mut retry_delay = 1;
    let max_retries = 5;

    for attempt in 1..=max_retries {
        let artwork_url = format!("https://www.pixiv.net/ajax/illust/{}", artwork_id);
        println!("Fetching artwork: {} (Attempt {})", artwork_url, attempt);

        let res = client
            .get(&artwork_url)
            .header(header::COOKIE, cookie)
            .header(header::REFERER, "https://www.pixiv.net/")
            .send()
            .await?;

        if res.status() == 429 {
            println!("Rate limited. Waiting for {} seconds before retry.", retry_delay);
            sleep(Duration::from_secs(retry_delay)).await;
            retry_delay *= 2;
            continue;
        }

        if !res.status().is_success() {
            return Err(format!("Failed to fetch artwork data: {}", res.status()).into());
        }

        let json: Value = res.json().await?;

        let title = json["body"]["title"].as_str().unwrap_or("untitled");
        let page_count = json["body"]["pageCount"].as_i64().unwrap_or(1) as usize;
        let is_r18 = json["body"]["xRestrict"].as_i64().unwrap_or(0) == 1;

        let folder_name = if is_r18 { "R18" } else { "All" };
        fs::create_dir_all(format!("downloads/{}", folder_name))?;

        for page in 0..page_count {
            let url = if page_count > 1 {
                json["body"]["urls"]["original"].as_str().unwrap().replace("_p0", &format!("_p{}", page))
            } else {
                json["body"]["urls"]["original"].as_str().unwrap().to_string()
            };

            let filename = Path::new(&url).file_name().unwrap().to_str().unwrap();
            let file_path = format!("downloads/{}/{}_{}", folder_name, title, filename);

            if Path::new(&file_path).exists() {
                println!("File already exists, skipping: {}", filename);
                continue;
            }

            println!("Downloading: {}", url);

            let image_res = client
                .get(&url)
                .header(header::COOKIE, cookie)
                .header(header::REFERER, "https://www.pixiv.net/")
                .send()
                .await?;

            if !image_res.status().is_success() {
                return Err(format!("Failed to download image: {}", image_res.status()).into());
            }

            let bytes = image_res.bytes().await?;

            let mut file = fs::File::create(&file_path)?;
            io::copy(&mut bytes.as_ref(), &mut file)?;
            println!("Successfully downloaded: {}", filename);
        }

        // 成功下载后，添加一个短暂的延迟
        sleep(Duration::from_millis(500)).await;
        return Ok(());
    }

    Err("Max retries reached. Unable to download artwork.".into())
}