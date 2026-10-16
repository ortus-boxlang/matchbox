//! Cookie wire validation and script-visible scope assignment share one path.
use chrono::{DateTime, Utc};
use http::HeaderValue;
use matchbox_vm::{
    types::{BxVM, BxValue, NativeCallback},
    vm::VM,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

#[derive(Clone, Debug)]
pub(crate) struct Cookie {
    pub name: String,
    pub value: String,
    pub path: String,
    pub domain: Option<String>,
    pub max_age: Option<i64>,
    pub expires: Option<String>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
    pub encode: bool,
}
impl Cookie {
    pub fn new(name: String, value: String) -> Self {
        Self {
            name,
            value,
            path: "/".into(),
            domain: None,
            max_age: None,
            expires: None,
            secure: false,
            http_only: false,
            same_site: None,
            encode: true,
        }
    }
    pub fn header(&self) -> Result<String, String> {
        if self.name.is_empty()
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        {
            return Err("Invalid cookie name".into());
        }
        for value in std::iter::once(self.path.as_str()).chain(self.domain.as_deref()) {
            if value.contains(';')
                || value
                    .bytes()
                    .any(|byte| byte < 32 || byte == 127 || byte > 126)
            {
                return Err("Invalid cookie path/domain".into());
            }
        }
        let value = if self.encode {
            url::form_urlencoded::byte_serialize(self.value.as_bytes()).collect()
        } else {
            if self
                .value
                .bytes()
                .any(|byte| byte <= 32 || byte >= 127 || b"\";,\\".contains(&byte))
            {
                return Err("Unencoded cookie value contains unsafe characters".into());
            }
            self.value.clone()
        };
        let mut result = format!("{}={value}; Path={}", self.name, self.path);
        if let Some(domain) = &self.domain {
            result.push_str(&format!("; Domain={domain}"));
        }
        if let Some(age) = self.max_age {
            result.push_str(&format!("; Max-Age={age}"));
        }
        if let Some(expires) = &self.expires {
            result.push_str(&format!("; Expires={expires}"));
        }
        if self.secure {
            result.push_str("; Secure");
        }
        if self.http_only {
            result.push_str("; HttpOnly");
        }
        if let Some(mode) = &self.same_site {
            result.push_str(&format!("; SameSite={mode}"));
        }
        HeaderValue::from_str(&result).map_err(|_| "Invalid cookie header")?;
        Ok(result)
    }
    pub fn from_value(vm: &mut dyn BxVM, name: String, value: BxValue) -> Result<Self, String> {
        let attributes = vm.is_struct_value(value).then(|| value.as_gc_id().unwrap());
        let get = |vm: &dyn BxVM, key: &str| {
            attributes
                .map(|id| vm.struct_get(id, key))
                .unwrap_or(BxValue::new_null())
        };
        let content = attributes
            .map(|id| vm.struct_get(id, "value"))
            .unwrap_or(value);
        let content = if content.is_null() {
            String::new()
        } else {
            let text = vm.cast_value_to_type(content, "string")?;
            vm.to_string(text)
        };
        let mut cookie = Self::new(name, content);
        for key in ["path", "domain", "sameSite"] {
            let value = get(vm, key);
            if !value.is_null() {
                let value = vm.to_string(value);
                match key {
                    "path" => cookie.path = value,
                    "domain" => cookie.domain = Some(value),
                    _ => {
                        cookie.same_site = match value.to_ascii_lowercase().as_str() {
                            "strict" => Some("Strict".into()),
                            "lax" => Some("Lax".into()),
                            "none" => Some("None".into()),
                            _ => {
                                eprintln!("Ignoring invalid cookie SameSite mode");
                                None
                            }
                        }
                    }
                }
            }
        }
        for key in ["secure", "httpOnly", "encodeValue"] {
            let value = get(vm, key);
            if !value.is_null() {
                let flag = vm.cast_value_to_type(value, "boolean")?.as_bool();
                match key {
                    "secure" => cookie.secure = flag,
                    "httpOnly" => cookie.http_only = flag,
                    _ => cookie.encode = flag,
                }
            }
        }
        let age = get(vm, "maxAge");
        if !age.is_null() {
            cookie.max_age = Some(
                vm.to_string(age)
                    .parse()
                    .map_err(|_| "Invalid cookie maxAge")?,
            );
        }
        let expires = get(vm, "expires");
        if !expires.is_null() {
            let text = vm.to_string(expires);
            if let Ok(days) = text.parse::<f64>() {
                if !days.is_finite() {
                    return Err("Invalid cookie expiration".into());
                }
                cookie.max_age =
                    Some((days * 86400.0).clamp(i32::MIN as f64, i32::MAX as f64) as i64);
            } else {
                match text.to_ascii_lowercase().as_str() {
                    "now" => cookie.max_age = Some(0),
                    "never" => cookie.max_age = Some(86400 * 365 * 30),
                    _ => {
                        let date = DateTime::parse_from_rfc3339(&text)
                            .or_else(|_| DateTime::parse_from_rfc2822(&text))
                            .map_err(|_| "Invalid cookie expiration date")?;
                        cookie.expires = Some(
                            date.with_timezone(&Utc)
                                .format("%a, %d %b %Y %H:%M:%S GMT")
                                .to_string(),
                        );
                    }
                }
            }
        }
        cookie.header()?;
        Ok(cookie)
    }
}
#[derive(Default, Debug)]
pub(crate) struct CookieJar {
    values: BTreeMap<(String, String, Option<String>), String>,
}
impl CookieJar {
    pub fn set(&mut self, cookie: Cookie) -> Result<(), String> {
        let header = cookie.header()?;
        self.values
            .insert((cookie.name, cookie.path, cookie.domain), header);
        Ok(())
    }
    pub fn headers(&self) -> impl Iterator<Item = String> + '_ {
        self.values.values().cloned()
    }
}
pub(crate) fn install(
    vm: &mut VM,
    incoming: &HashMap<String, String>,
    jar: Rc<RefCell<CookieJar>>,
) -> Result<(), String> {
    let scope = vm.insert_empty_struct_global("cookie").as_gc_id().unwrap();
    for (name, value) in incoming {
        let value = BxValue::new_ptr(vm.string_new(value.clone()));
        vm.struct_set(scope, name, value);
    }
    vm.watch_struct_assignments(
        scope,
        NativeCallback {
            parameters: vec!["name".into(), "value".into()],
            callback: Rc::new(move |vm, args| {
                let name = vm.to_string(args[0]);
                let cookie = Cookie::from_value(vm, name, args[1])?;
                let value = BxValue::new_ptr(vm.string_new(cookie.value.clone()));
                jar.borrow_mut().set(cookie)?;
                Ok(value)
            }),
        },
    )
}
