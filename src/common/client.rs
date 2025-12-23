use crate::app::{
    constant::{
        CURSOR_API2_HOST, CURSOR_HOST,
        header::{
            AMZN_TRACE_ID, CLIENT_KEY, CONNECT_ACCEPT_ENCODING, CONNECT_CONTENT_ENCODING,
            CONNECT_ES, CONNECT_PROTO, CONNECT_PROTOCOL_VERSION, CORS, CURSOR_CHECKSUM,
            CURSOR_CLIENT_VERSION, CURSOR_CONFIG_VERSION, CURSOR_ORIGIN, CURSOR_REFERER_URL,
            CURSOR_STREAMING, CURSOR_TIMEZONE, EMPTY, ENCODING, ENCODINGS, FALSE, FS_CLIENT_KEY,
            GHOST_MODE, HEADER_VALUE_ACCEPT, JSON, KEEP_ALIVE, LANGUAGE, NEW_ONBOARDING_COMPLETED,
            NO_CACHE, NONE, ONE, PRIORITY, PROTO, PROXY_HOST, REQUEST_ID, SAME_ORIGIN,
            SEC_FETCH_DEST, SEC_FETCH_MODE, SEC_FETCH_SITE, SEC_GPC, SESSION_ID, TRAILERS, TRUE,
            U_EQ_0, UA, VSCODE_ORIGIN, cursor_client_version, header_value_ua_cursor_latest,
        },
    },
    lazy::{
        PRI_REVERSE_PROXY_HOST, PUB_REVERSE_PROXY_HOST, USE_PRI_REVERSE_PROXY,
        USE_PUB_REVERSE_PROXY, sessions_url, stripe_url, token_poll_url, token_refresh_url,
        token_upgrade_url, usage_api_url,
    },
    model::ExtToken,
};
use http::{
    header::{
        ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, AUTHORIZATION, CACHE_CONTROL, CONNECTION,
        CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, DNT, HOST, ORIGIN, PRAGMA, REFERER,
        TE, USER_AGENT,
    },
    method::Method,
};
use reqwest::{Client, RequestBuilder};

trait RequestBuilderExt: Sized {
    fn opt_header<K, V>(self, key: K, value: Option<V>) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>;

    fn opt_header_map<K, I, V, F: FnOnce(I) -> V>(self, key: K, value: Option<I>, f: F) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>;

    fn header_if<K, V>(self, key: K, value: V, condition: bool) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>;

    fn header_map<K, I, V, F: FnOnce(I) -> V>(self, key: K, value: I, f: F) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>;
}

impl RequestBuilderExt for RequestBuilder {
    #[inline]
    fn opt_header<K, V>(self, key: K, value: Option<V>) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        if let Some(value) = value { self.header(key, value) } else { self }
    }

    #[inline]
    fn opt_header_map<K, I, V, F: FnOnce(I) -> V>(self, key: K, value: Option<I>, f: F) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        if let Some(value) = value { self.header(key, f(value)) } else { self }
    }

    #[inline]
    fn header_if<K, V>(self, key: K, value: V, condition: bool) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        if condition { self.header(key, value) } else { self }
    }

    #[inline]
    fn header_map<K, I, V, F: FnOnce(I) -> V>(self, key: K, value: I, f: F) -> Self
    where
        http::HeaderName: TryFrom<K>,
        <http::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::HeaderValue: TryFrom<V>,
        <http::HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        self.header(key, f(value))
    }
}

#[inline]
fn get_client_and_host<'a>(
    client: &Client,
    method: Method,
    url: &'a str,
    use_pri: bool,
    real_host: &'a str,
) -> (RequestBuilder, &'a str) {
    if use_pri && *USE_PRI_REVERSE_PROXY {
        (client.request(method, url).header(PROXY_HOST, real_host), &PRI_REVERSE_PROXY_HOST)
    } else if !use_pri && *USE_PUB_REVERSE_PROXY {
        (client.request(method, url).header(PROXY_HOST, real_host), &PUB_REVERSE_PROXY_HOST)
    } else {
        (client.request(method, url), real_host)
    }
}

pub(crate) struct AiServiceRequest<'a> {
    pub(crate) ext_token: &'a ExtToken,
    pub(crate) fs_client_key: Option<http::HeaderValue>,
    pub(crate) url: &'static str,
    pub(crate) stream: bool,
    pub(crate) compressed: bool,
    pub(crate) trace_id: [u8; 36],
    pub(crate) use_pri: bool,
    pub(crate) cookie: Option<http::HeaderValue>,
}

pub fn build_client_request(req: AiServiceRequest) -> RequestBuilder {
    let (builder, host) = get_client_and_host(
        &req.ext_token.get_client(),
        Method::POST,
        req.url,
        req.use_pri,
        CURSOR_API2_HOST,
    );

    let mut buf = [0u8; 137];

    builder
        .version(http::Version::HTTP_2)
        .header(HOST, host)
        .header_if(ACCEPT_ENCODING, ENCODING, !req.stream)
        .header_if(CONTENT_ENCODING, ENCODING, !req.stream && req.compressed)
        .header(AUTHORIZATION, req.ext_token.as_unext().format_bearer_token())
        .header_if(CONNECT_ACCEPT_ENCODING, ENCODING, req.stream)
        .header_if(CONNECT_CONTENT_ENCODING, ENCODING, req.stream)
        .header(CONNECT_PROTOCOL_VERSION, ONE)
        .header(CONTENT_TYPE, if req.stream { CONNECT_PROTO } else { PROTO })
        .header(COOKIE, req.cookie.unwrap_or(NONE))
        .header(USER_AGENT, CONNECT_ES)
        .header_map(AMZN_TRACE_ID, req.trace_id, |v| {
            const PREFIX: &[u8; 5] = b"Root=";
            unsafe {
                core::ptr::copy_nonoverlapping(PREFIX.as_ptr(), buf.as_mut_ptr(), 5);
                core::ptr::copy_nonoverlapping(v.as_ptr(), buf.as_mut_ptr().add(5), 36);
                http::HeaderValue::from_bytes(buf.get_unchecked(..41)).unwrap_unchecked()
            }
        })
        .header(CLIENT_KEY, unsafe {
            http::HeaderValue::from_bytes({
                req.ext_token.client_key.to_str(&mut *(buf.as_mut_ptr() as *mut [u8; 64]));
                buf.get_unchecked(..64)
            })
            .unwrap_unchecked()
        })
        .header(
            CURSOR_CHECKSUM,
            __unwrap!(http::HeaderValue::from_bytes({
                req.ext_token.checksum.to_str(&mut buf);
                &buf
            })),
        )
        .header(CURSOR_CLIENT_VERSION, cursor_client_version())
        .opt_header_map(CURSOR_CONFIG_VERSION, req.ext_token.config_version, |v| {
            v.hyphenated().encode_lower(unsafe { &mut *(buf.as_mut_ptr() as *mut [u8; 36]) });
            unsafe { http::HeaderValue::from_bytes(buf.get_unchecked(..36)).unwrap_unchecked() }
        })
        .header(CURSOR_STREAMING, TRUE)
        .header(CURSOR_TIMEZONE, req.ext_token.timezone_name())
        .opt_header(FS_CLIENT_KEY, req.fs_client_key)
        .header(GHOST_MODE, TRUE)
        .header(NEW_ONBOARDING_COMPLETED, FALSE)
        .header_map(REQUEST_ID, req.trace_id, |v| __unwrap!(http::HeaderValue::from_bytes(&v)))
        .header(SESSION_ID, {
            req.ext_token
                .session_id
                .hyphenated()
                .encode_lower(unsafe { &mut *(buf.as_mut_ptr() as *mut [u8; 36]) });
            unsafe { http::HeaderValue::from_bytes(buf.get_unchecked(..36)).unwrap_unchecked() }
        })
}

pub fn build_stripe_request(
    client: &Client,
    bearer_token: http::HeaderValue,
    use_pri: bool,
) -> RequestBuilder {
    let (builder, host) =
        get_client_and_host(client, Method::GET, stripe_url(use_pri), use_pri, CURSOR_API2_HOST);

    builder
        .version(http::Version::HTTP_2)
        .header(HOST, host)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(AUTHORIZATION, bearer_token)
        .header(GHOST_MODE, TRUE)
        .header(NEW_ONBOARDING_COMPLETED, FALSE)
        .header(USER_AGENT, header_value_ua_cursor_latest())
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ORIGIN, VSCODE_ORIGIN)
    // .header(SEC_CH_UA, NOT_A_BRAND)
    // .header(SEC_CH_UA_MOBILE, MOBILE_NO)
    // .header(SEC_CH_UA_PLATFORM, WINDOWS)
    // .header(SEC_FETCH_SITE, CROSS_SITE)
    // .header(SEC_FETCH_MODE, CORS)
    // .header(SEC_FETCH_DEST, EMPTY)
    // .header(SEC_GPC, ONE)
    // .header(CONNECTION, KEEP_ALIVE)
    // .header(PRAGMA, NO_CACHE)
    // .header(CACHE_CONTROL, NO_CACHE)
    // .header(TE, TRAILERS)
    // .header(PRIORITY, U_EQ_0)
}

pub fn build_usage_request(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> RequestBuilder {
    let (builder, host) =
        get_client_and_host(client, Method::GET, usage_api_url(use_pri), use_pri, CURSOR_HOST);

    builder
        .header(HOST, host)
        .header(USER_AGENT, UA)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(REFERER, CURSOR_REFERER_URL)
        .header(DNT, ONE)
        .header(SEC_GPC, ONE)
        .header(CONNECTION, KEEP_ALIVE)
        .header(COOKIE, cookie)
        .header(SEC_FETCH_DEST, EMPTY)
        .header(SEC_FETCH_MODE, CORS)
        .header(SEC_FETCH_SITE, SAME_ORIGIN)
        .header(PRIORITY, U_EQ_0)
        .header(PRAGMA, NO_CACHE)
        .header(CACHE_CONTROL, NO_CACHE)
}

// pub fn build_userinfo_request(
//     client: &Client,
//     cookie: http::HeaderValue,
//     use_pri: bool,
// ) -> RequestBuilder {
//     let (builder, host) = get_client_and_host(
//         client,
//         Method::POST,
//         user_api_url(use_pri),
//         use_pri,
//         CURSOR_HOST,
//     );

//     builder
//         .header(HOST, host)
//         .header(USER_AGENT, UA)
//         .header(ACCEPT, HEADER_VALUE_ACCEPT)
//         .header(ACCEPT_LANGUAGE, LANGUAGE)
//         .header(ACCEPT_ENCODING, ENCODINGS)
//         .header(REFERER, CURSOR_REFERER_URL)
//         .header(DNT, ONE)
//         .header(SEC_GPC, ONE)
//         .header(CONNECTION, KEEP_ALIVE)
//         .header(COOKIE, cookie)
//         .header(SEC_FETCH_DEST, EMPTY)
//         .header(SEC_FETCH_MODE, CORS)
//         .header(SEC_FETCH_SITE, SAME_ORIGIN)
//         .header(PRAGMA, NO_CACHE)
//         .header(CACHE_CONTROL, NO_CACHE)
//         .header(TE, TRAILERS)
//         .header(PRIORITY, U_EQ_0)
// }

pub fn build_token_upgrade_request(
    client: &Client,
    uuid: &str,
    challenge: &str,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> RequestBuilder {
    let (builder, host) =
        get_client_and_host(client, Method::POST, token_upgrade_url(use_pri), use_pri, CURSOR_HOST);

    crate::define_typed_constants! {
        &'static str => {
            UUID_PREFIX = "{\"uuid\":\"",
            CHALLENGE_PREFIX = "\",\"challenge\":\"",
            SUFFIX = "\"}",

            REFERER_PREFIX = "https://cursor.com/loginDeepControl?challenge=",
            REFERER_MIDDLE = "&uuid=",
            REFERER_SUFFIX = "&mode=login",
        }
        usize => {
            UUID_LEN = 36,
            CHALLENGE_LEN = 43,

            BODY_CAPACITY = UUID_PREFIX.len() + UUID_LEN + CHALLENGE_PREFIX.len() + CHALLENGE_LEN + SUFFIX.len(),
            REFERER_CAPACITY = REFERER_PREFIX.len() + CHALLENGE_LEN + REFERER_MIDDLE.len() + UUID_LEN + REFERER_SUFFIX.len(),
        }
    }

    // 使用常量预分配空间 - body
    let mut body = String::with_capacity(BODY_CAPACITY);
    body.push_str(UUID_PREFIX);
    body.push_str(uuid);
    body.push_str(CHALLENGE_PREFIX);
    body.push_str(challenge);
    body.push_str(SUFFIX);

    // 使用常量预分配空间 - referer
    let mut referer = String::with_capacity(REFERER_CAPACITY);
    referer.push_str(REFERER_PREFIX);
    referer.push_str(challenge);
    referer.push_str(REFERER_MIDDLE);
    referer.push_str(uuid);
    referer.push_str(REFERER_SUFFIX);

    builder
        .header(HOST, host)
        .header(USER_AGENT, UA)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(REFERER, referer)
        .header(CONTENT_TYPE, JSON)
        .header(CONTENT_LENGTH, body.len())
        .header(DNT, ONE)
        .header(SEC_GPC, ONE)
        .header(CONNECTION, KEEP_ALIVE)
        .header(COOKIE, cookie)
        .header(SEC_FETCH_DEST, EMPTY)
        .header(SEC_FETCH_MODE, CORS)
        .header(SEC_FETCH_SITE, SAME_ORIGIN)
        .header(PRAGMA, NO_CACHE)
        .header(CACHE_CONTROL, NO_CACHE)
        .header(TE, TRAILERS)
        .header(PRIORITY, U_EQ_0)
        .body(body)
}

pub fn build_token_poll_request(
    client: &Client,
    uuid: &str,
    verifier: &str,
    use_pri: bool,
) -> RequestBuilder {
    let (builder, host) = get_client_and_host(
        client,
        Method::GET,
        token_poll_url(use_pri),
        use_pri,
        CURSOR_API2_HOST,
    );

    builder
        .version(http::Version::HTTP_11)
        .header(HOST, host)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(USER_AGENT, header_value_ua_cursor_latest())
        .header(ORIGIN, VSCODE_ORIGIN)
        .header(GHOST_MODE, TRUE)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .query(&[("uuid", uuid), ("verifier", verifier)])
}

pub fn build_token_refresh_request(
    client: &Client,
    use_pri: bool,
    body: Vec<u8>,
) -> RequestBuilder {
    let (builder, host) = get_client_and_host(
        client,
        Method::POST,
        token_refresh_url(use_pri),
        use_pri,
        CURSOR_API2_HOST,
    );

    builder
        .header(HOST, host)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(CONTENT_TYPE, JSON)
        .header(CONTENT_LENGTH, body.len())
        .header(USER_AGENT, header_value_ua_cursor_latest())
        .header(ORIGIN, VSCODE_ORIGIN)
        .header(GHOST_MODE, TRUE)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .body(body)
}

pub fn build_proto_web_request(
    client: &Client,
    cookie: http::HeaderValue,
    url: &'static str,
    use_pri: bool,
    body: bytes::Bytes,
) -> RequestBuilder {
    let (builder, host) = get_client_and_host(client, Method::POST, url, use_pri, CURSOR_HOST);

    builder
        .header(HOST, host)
        .header(USER_AGENT, UA)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(REFERER, CURSOR_REFERER_URL)
        .header(CONTENT_TYPE, JSON)
        .header(CONTENT_LENGTH, body.len())
        .header(ORIGIN, CURSOR_ORIGIN)
        .header(DNT, ONE)
        .header(SEC_GPC, ONE)
        .header(CONNECTION, KEEP_ALIVE)
        .header(COOKIE, cookie)
        .header(SEC_FETCH_DEST, EMPTY)
        .header(SEC_FETCH_MODE, CORS)
        .header(SEC_FETCH_SITE, SAME_ORIGIN)
        .header(PRIORITY, U_EQ_0)
        .header(PRAGMA, NO_CACHE)
        .header(CACHE_CONTROL, NO_CACHE)
        .header(TE, TRAILERS)
        .body(body)
}

pub fn build_sessions_request(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> RequestBuilder {
    let (builder, host) =
        get_client_and_host(client, Method::GET, sessions_url(use_pri), use_pri, CURSOR_HOST);

    builder
        .header(HOST, host)
        .header(USER_AGENT, UA)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(REFERER, CURSOR_REFERER_URL)
        .header(DNT, ONE)
        .header(SEC_GPC, ONE)
        .header(CONNECTION, KEEP_ALIVE)
        .header(COOKIE, cookie)
        .header(SEC_FETCH_DEST, EMPTY)
        .header(SEC_FETCH_MODE, CORS)
        .header(SEC_FETCH_SITE, SAME_ORIGIN)
        .header(PRAGMA, NO_CACHE)
        .header(CACHE_CONTROL, NO_CACHE)
        .header(TE, TRAILERS)
        .header(PRIORITY, U_EQ_0)
}
