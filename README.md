# Cloudflare Workers Next.js Media Proxy

A fast, simple image media proxy written in Rust that runs on Cloudflare Workers.

## Features

- Supports passthrough of SVG images
- Supports PNG, JPEG, and WebP images
- Supports lossy and lossless WebP images
- Passes through ICC color profiles embedded inside images

## Usage

To use the media proxy, make a request to the following endpoint:

```
/{base64url-encoded-url}?{query-parameters}
```

### Supported Query Parameters

- `q`: Quality (0-100, default: 85)
- `w`: Max width (default: `DEFAULT_MAX_WIDTH`)
- `h`: Max height (default: `DEFAULT_MAX_HEIGHT`)
- `f`: Format (png, jpeg, webp)

### Example

```sh
curl "https://your-cloudflare-worker-url/$(echo -n 'https://example.com/image.jpg' | basenc --base64url)?w=800&h=600&q=90&f=webp"
```

## Integration with Next.js

To integrate the media proxy with Next.js, you must add a custom image loader that generates the correct URL for the media proxy.

Example implementation:

```javascript
'use client'
 
export default function cfWorkerImageLoader({ src, width, quality }) {
    const base64Url = btoa(src).replace('+','-').replace('/','_').replace('=','');
    return `https://your-cloudflare-worker-url/${base64Url}?w=${width}&f=webp${quality ? '&q=' + quality : ''}`;
}
```

## Configuration

1. Copy `wrangler.toml.example` to `wrangler.toml`:

```sh
cp wrangler.toml.example wrangler.toml
```

2. Set the allowed domains in `wrangler.toml`:

```toml
[vars]
DOMAIN_WHITELIST = "example.com,anotherdomain.com"
```

3. Deploy the worker:

```sh
npx wrangler deploy
```

## License

This project is licensed under the MIT License.