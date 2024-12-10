use url::Url;
use worker::*;

#[cfg(target_family = "wasm")]
mod wasm;

pub mod processor;
pub mod util;

fn make_image_params(url: &Url) -> processor::ImageParams {
    use processor::*;

    let query_pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();

    ImageParams {
        quality: query_pairs
            .get("q")
            .and_then(|q| q.parse().ok())
            .map(|q: u8| q.min(100))
            .unwrap_or(85),
        max_width: query_pairs
            .get("w")
            .and_then(|w| w.parse().ok())
            .map(|w: u32| w.min(DEFAULT_MAX_WIDTH)),
        max_height: query_pairs
            .get("h")
            .and_then(|h| h.parse().ok())
            .map(|h: u32| h.min(DEFAULT_MAX_HEIGHT)),
        format: query_pairs.get("f").map(|f| ImageType::from(f as &str)),
    }
}

fn respond_cache(
    req: Request,
    mut response: Response,
    cache: Cache,
    ctx: &Context,
) -> Result<Response> {
    let cache_response = response.cloned()?;
    ctx.wait_until(async move {
        let _ = cache.put(&req, cache_response).await;
    });

    return Ok(response);
}

fn check_domain_whitelist(url: &Url, whitelist: &str) -> bool {
    if whitelist == "*" {
        return true;
    }

    let host = url.host_str().unwrap_or_default();
    whitelist.split(',').any(|pattern| {
        if pattern.starts_with("*.") {
            host.ends_with(&pattern[1..])
        } else {
            host == pattern
        }
    })
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, ctx: Context) -> Result<Response> {
    use processor::*;

    console_error_panic_hook::set_once();

    let url = req.url()?;
    let path = url.path().trim_start_matches('/');

    // Decode the base64 URL from the path
    let img_url = util::decode_base64_non_strict(path)
        .ok()
        .and_then(|s| Url::parse(&s).ok());
    let img_url = if let Some(url) = img_url {
        url
    } else {
        return Response::error("Invalid URL", 400);
    };

    // Check domain whitelist
    let whitelist = env
        .var("DOMAIN_WHITELIST")
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "*".to_string());

    if !check_domain_whitelist(&img_url, &whitelist) {
        return Response::error("Domain not allowed", 403);
    }

    let params = make_image_params(&url);
    let cache = Cache::default();

    if let Some(cached_response) = cache.get(&req, true).await? {
        return Ok(cached_response);
    }

    let mut img_response = Fetch::Url(img_url).send().await?;
    let img_status_code = img_response.status_code();

    if img_status_code < 200 || img_status_code >= 300 {
        // sanitize weird status codes between 400 and 599
        let resp_status_code = img_status_code.min(599).max(400);

        return Response::error(
            format!("Upstream server returned {}", resp_status_code),
            resp_status_code,
        );
    }

    let img_data = img_response.bytes().await?;
    let src_format = if let Some(format) = ImageType::detect_image_format(&img_data) {
        format
    } else {
        return Response::error("Invalid image format", 400);
    };
    let cache_control = "public, max-age=31536000";

    if src_format == ImageType::Svg {
        let mut response = Response::from_bytes(img_data)?;
        response
            .headers_mut()
            .set("Content-Type", src_format.to_mime())?;
        response.headers_mut().set("Cache-Control", cache_control)?;
        return respond_cache(req, response, cache, &ctx);
    }

    let result = match process_image(img_data, src_format, &params) {
        Ok(result) => result,
        Err(e) => return Response::error(e.to_string(), 500),
    };

    let mut response = Response::from_bytes(result.data)?;

    response
        .headers_mut()
        .set("Content-Type", result.format.to_mime())?;
    response.headers_mut().set("Cache-Control", cache_control)?;

    respond_cache(req, response, cache, &ctx)
}
