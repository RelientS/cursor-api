use super::model::HeaderValue;
use crate::app::{
    constant::{
        CURSOR_API2_HOST, CURSOR_HOST,
        header::{
            AMZN_TRACE_ID, CLIENT_KEY, CLOSE, CONNECT_ACCEPT_ENCODING, CONNECT_CONTENT_ENCODING,
            CONNECT_ES, CONNECT_PROTO, CONNECT_PROTOCOL_VERSION, CONNECTION, CORS, CROSS_SITE,
            CURSOR_CHECKSUM, CURSOR_CLIENT_VERSION, CURSOR_CONFIG_VERSION, CURSOR_ORIGIN,
            CURSOR_REFERER_URL, CURSOR_STREAMING, CURSOR_TIMEZONE, EMPTY, ENCODING, ENCODINGS,
            FALSE, FS_CLIENT_KEY, GHOST_MODE, HEADER_VALUE_ACCEPT, HOST, JSON, KEEP_ALIVE,
            LANGUAGE, MOBILE_NO, NEW_ONBOARDING_COMPLETED, NO_CACHE, NONE, NOT_A_BRAND, ONE,
            PRIORITY, PROTO, PROXY_HOST, REQUEST_ID, SAME_ORIGIN, SEC_CH_UA, SEC_CH_UA_MOBILE,
            SEC_CH_UA_PLATFORM, SEC_FETCH_DEST, SEC_FETCH_MODE, SEC_FETCH_SITE, SEC_GPC,
            SESSION_ID, TRAILERS, TRUE, U_EQ_0, U_EQ_1_I, VSCODE_ORIGIN, ZERO,
        },
    },
    lazy::{
        USE_PRI_REVERSE_PROXY, USE_PUB_REVERSE_PROXY, pri_reverse_proxy_host,
        pub_reverse_proxy_host, sessions_url, stripe_url, token_refresh_url, token_upgrade_url,
        usage_api_url,
    },
    model::{ExtToken, cursor_version, platform},
};
use http::{
    header::{
        ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, AUTHORIZATION, CACHE_CONTROL, CONTENT_ENCODING,
        CONTENT_LENGTH, CONTENT_TYPE, COOKIE, DNT, ORIGIN, PRAGMA, REFERER, TE, USER_AGENT,
    },
    method::Method,
};
use reqwest::{Client, RequestBuilder};
use url::Url;

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

    // fn version_if(self, version: http::Version, condition: bool) -> Self;

    fn when<F>(self, condition: bool, f: F) -> Self
    where F: FnOnce(Self) -> Self;
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

    // #[inline]
    // fn version_if(self, version: http::Version, condition: bool) -> Self {
    //     if condition { self.version(version) } else { self }
    // }

    #[inline]
    fn when<F>(self, condition: bool, f: F) -> Self
    where F: FnOnce(Self) -> Self {
        if condition { f(self) } else { self }
    }
}

#[inline]
fn get_client(
    client: &Client,
    method: Method,
    url: Url,
    use_pri: bool,
    real_host: &'static str,
) -> RequestBuilder {
    if (use_pri && unsafe { USE_PRI_REVERSE_PROXY }) || (!use_pri && unsafe { USE_PUB_REVERSE_PROXY }) {
        client
            .request(method, url)
            .header(PROXY_HOST, unsafe { super::model::HeaderValue::from_static(real_host).into() })
    } else {
        client.request(method, url)
    }
}

#[inline]
fn get_client_and_host(
    client: &Client,
    method: Method,
    url: Url,
    use_pri: bool,
    real_host: &'static str,
) -> (RequestBuilder, http::header::HeaderValue) {
    if use_pri && unsafe { USE_PRI_REVERSE_PROXY } {
        (client.request(method, url).header(PROXY_HOST, real_host), pri_reverse_proxy_host())
    } else if !use_pri && unsafe { USE_PUB_REVERSE_PROXY } {
        (client.request(method, url).header(PROXY_HOST, real_host), pub_reverse_proxy_host())
    } else {
        (client.request(method, url), unsafe {
            super::model::HeaderValue::from_static(real_host).into()
        })
    }
}

pub(crate) struct AiServiceRequest<'a> {
    pub ext_token: &'a ExtToken,
    pub fs_client_key: Option<http::HeaderValue>,
    pub url: &'static Url,
    pub stream: bool,
    pub compressed: bool,
    pub trace_id: [u8; 36],
    pub use_pri: bool,
    pub cookie: Option<http::HeaderValue>,
    pub exact_length: Option<usize>,
}

pub fn build_client_request(req: AiServiceRequest) -> RequestBuilder {
    let builder = get_client(
        &req.ext_token.get_client(),
        Method::POST,
        req.url.clone(),
        req.use_pri,
        CURSOR_API2_HOST,
    );

    let mut buf = [0u8; 137];

    builder
        // .version_if(http::version::Version::HTTP_2, req.is_http2)
        .version(http::version::Version::HTTP_2)
        .header_if(ACCEPT_ENCODING, ENCODING, !req.stream)
        .header(AUTHORIZATION, req.ext_token.as_unext().format_bearer_token())
        .when(req.stream, |builder| {
            builder
                .header(CONNECT_ACCEPT_ENCODING, ENCODING)
                .header(CONNECT_CONTENT_ENCODING, ENCODING)
        })
        .header_if(CONTENT_ENCODING, ENCODING, !req.stream && req.compressed)
        .header(CONNECT_PROTOCOL_VERSION, ONE)
        // placeholder - to be replaced
        .opt_header_map(CONTENT_LENGTH, req.exact_length, |v| unsafe {
            HeaderValue::from_bytes(itoa::Buffer::new().format(v).as_bytes()).into()
        })
        .header(CONTENT_TYPE, if req.stream { CONNECT_PROTO } else { PROTO })
        .header(COOKIE, req.cookie.unwrap_or(NONE))
        // skip traceparent
        .header(USER_AGENT, CONNECT_ES)
        .header_map(AMZN_TRACE_ID, req.trace_id, |v| {
            const PREFIX: &[u8; 5] = b"Root=";
            unsafe {
                core::ptr::copy_nonoverlapping(PREFIX.as_ptr(), buf.as_mut_ptr(), 5);
                core::ptr::write(buf.as_mut_ptr().add(5).cast(), v);
                HeaderValue::from_bytes(buf.get_unchecked(..41)).into()
            }
        })
        .header(CLIENT_KEY, unsafe {
            req.ext_token.client_key.to_str(&mut *(buf.as_mut_ptr() as *mut [u8; 64]));
            HeaderValue::from_bytes(buf.get_unchecked(..64)).into()
        })
        .header(CURSOR_CHECKSUM, unsafe {
            req.ext_token.checksum.to_str(&mut buf);
            HeaderValue::from_bytes(&buf).into()
        })
        .header(CURSOR_CLIENT_VERSION, cursor_version::client_version())
        .opt_header_map(CURSOR_CONFIG_VERSION, req.ext_token.config_version, |v| {
            v.hyphenated().encode_lower(unsafe { &mut *(buf.as_mut_ptr() as *mut [u8; 36]) });
            unsafe { HeaderValue::from_bytes(buf.get_unchecked(..36)).into() }
        })
        .header(CURSOR_STREAMING, TRUE)
        .header(CURSOR_TIMEZONE, req.ext_token.timezone_as_header_value())
        .opt_header(FS_CLIENT_KEY, req.fs_client_key)
        .header(GHOST_MODE, TRUE)
        .header(NEW_ONBOARDING_COMPLETED, FALSE)
        .header_map(REQUEST_ID, req.trace_id, |v| unsafe { HeaderValue::from_bytes(&v).into() })
        .header(SESSION_ID, {
            req.ext_token
                .session_id
                .hyphenated()
                .encode_lower(unsafe { &mut *(buf.as_mut_ptr() as *mut [u8; 36]) });
            unsafe { HeaderValue::from_bytes(buf.get_unchecked(..36)).into() }
        })
    // .when(!req.is_http2, |builder| {
    //     builder.header(HOST, host).header(CONNECTION, CLOSE).header_if(
    //         TRANSFER_ENCODING,
    //         CHUNKED,
    //         req.exact_length.is_none(),
    //     )
    // })
}

pub fn build_stripe_request(
    client: &Client,
    bearer_token: http::HeaderValue,
    use_pri: bool,
) -> RequestBuilder {
    let builder =
        get_client(client, Method::GET, stripe_url(use_pri).clone(), use_pri, CURSOR_API2_HOST);

    builder
        .version(http::Version::HTTP_2)
        .header(SEC_CH_UA_PLATFORM, platform::current().as_header_value())
        .header(AUTHORIZATION, bearer_token)
        .header(CURSOR_CLIENT_VERSION, cursor_version::client_version())
        .header(NEW_ONBOARDING_COMPLETED, FALSE)
        .header(SEC_CH_UA, NOT_A_BRAND)
        .header(SEC_CH_UA_MOBILE, MOBILE_NO)
        // skip traceparent
        .header(GHOST_MODE, TRUE)
        .header(USER_AGENT, cursor_version::ua_cursor())
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ORIGIN, VSCODE_ORIGIN)
        .header(SEC_FETCH_SITE, CROSS_SITE)
        .header(SEC_FETCH_MODE, CORS)
        .header(SEC_FETCH_DEST, EMPTY)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(PRIORITY, U_EQ_1_I)
    // .header(SEC_GPC, ONE)
    // .header(CONNECTION, KEEP_ALIVE)
    // .header(PRAGMA, NO_CACHE)
    // .header(CACHE_CONTROL, NO_CACHE)
    // .header(TE, TRAILERS)
}

pub fn build_usage_request(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> RequestBuilder {
    let builder =
        get_client(client, Method::GET, usage_api_url(use_pri).clone(), use_pri, CURSOR_HOST);

    builder
        .version(http::Version::HTTP_2)
        // .header(HOST, host)
        .header(USER_AGENT, platform::current().web_ua())
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
    let builder =
        get_client(client, Method::POST, token_upgrade_url(use_pri).clone(), use_pri, CURSOR_HOST);

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
        .version(http::Version::HTTP_2)
        // .header(HOST, host)
        .header(USER_AGENT, platform::current().web_ua())
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(REFERER, unsafe { HeaderValue::from(referer).into() })
        .header(CONTENT_TYPE, JSON)
        .header(CONTENT_LENGTH, HeaderValue::from_integer(body.len()))
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

pub fn build_token_poll_request(client: &Client, url: Url, use_pri: bool) -> RequestBuilder {
    let (builder, host) = get_client_and_host(client, Method::GET, url, use_pri, CURSOR_API2_HOST);

    builder
        // .version(http::Version::HTTP_11)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(CONTENT_LENGTH, ZERO)
        .header(USER_AGENT, cursor_version::ua_cursor())
        .header(ORIGIN, VSCODE_ORIGIN)
        .header(GHOST_MODE, TRUE)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(HOST, host)
        .header(CONNECTION, CLOSE)
}

pub fn build_token_refresh_request(
    client: &Client,
    use_pri: bool,
    body: Vec<u8>,
) -> RequestBuilder {
    let builder = get_client(
        client,
        Method::POST,
        token_refresh_url(use_pri).clone(),
        use_pri,
        CURSOR_API2_HOST,
    );

    builder
        .version(http::Version::HTTP_2)
        // .header(HOST, host)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(CONTENT_TYPE, JSON)
        .header(CONTENT_LENGTH, HeaderValue::from_integer(body.len()))
        .header(USER_AGENT, cursor_version::ua_cursor())
        .header(ORIGIN, VSCODE_ORIGIN)
        .header(GHOST_MODE, TRUE)
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .body(body)
}

pub fn build_proto_web_request(
    client: &Client,
    cookie: http::HeaderValue,
    url: &'static Url,
    use_pri: bool,
    body: bytes::Bytes,
) -> RequestBuilder {
    let builder = get_client(client, Method::POST, url.clone(), use_pri, CURSOR_HOST);

    builder
        .version(http::Version::HTTP_2)
        // .header(HOST, host)
        .header(USER_AGENT, platform::current().web_ua())
        .header(ACCEPT, HEADER_VALUE_ACCEPT)
        .header(ACCEPT_LANGUAGE, LANGUAGE)
        .header(ACCEPT_ENCODING, ENCODINGS)
        .header(REFERER, CURSOR_REFERER_URL)
        .header(CONTENT_TYPE, JSON)
        .header(CONTENT_LENGTH, HeaderValue::from_integer(body.len()))
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
    let builder =
        get_client(client, Method::GET, sessions_url(use_pri).clone(), use_pri, CURSOR_HOST);

    builder
        .version(http::Version::HTTP_2)
        // .header(HOST, host)
        .header(USER_AGENT, platform::current().web_ua())
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
