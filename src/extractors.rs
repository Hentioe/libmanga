use crate::{error::*, models::*};
use duang::duang;
use encoding_rs::*;
use quick_js::{Context, JsValue};
use regex::Regex;
use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::vec::Vec;

pub use crate::helper::{document_ext::*, grouped_items::*, *};

macro_rules! def_bool_status {
    ( $(:$name:ident),* ) => {
        paste::item! {
            $(
                fn [<is_ $name>](&self) -> bool {
                    if let Some(item) = self.read_status().get(format!(stringify!($name)).as_str()) {
                        item.downcast_ref::<bool>() == Some(&true)
                    } else {
                        false
                    }
                }
            )*
        }
    };
}

macro_rules! def_status_access {
    ( $type:pat, $key:ident ) => {
        paste::item! {
            fn [<get_ $key>](&self) -> Option<&$type> {
                if let Some(item) = self.read_status().get(format!(stringify!($key)).as_str()) {
                    item.downcast_ref::<$type>()
                } else {
                    None
                }
            }
        }
    };
}

type Status = HashMap<&'static str, Box<dyn Any + Send + Sync>>;

#[allow(unused_variables)]
pub trait Extractor {
    def_bool_status![:usable, :searchable, :pageable, :pageable_search, :https];

    def_status_access!(&str, favicon);

    fn read_status(&self) -> &Status;

    fn tags(&self) -> &Vec<Tag>;

    fn index(&self, page: u32) -> Result<Vec<Comic>> {
        Ok(vec![])
    }

    fn fetch_chapters(&self, comic: &mut Comic) -> Result<()> {
        Ok(())
    }

    fn pages_iter<'a>(&'a self, chapter: &'a mut Chapter) -> Result<ChapterPages> {
        Ok(ChapterPages::new(
            chapter,
            0,
            vec![],
            Box::new(|_| Ok(vec![])),
        ))
    }

    fn fetch_pages(&self, chapter: &mut Chapter) -> Result<()> {
        self.pages_iter(chapter)?.for_each(drop);
        Ok(())
    }

    fn fetch_pages_unsafe(&self, chapter: &mut Chapter) -> Result<()> {
        self.pages_iter(chapter)?.for_each(|r| {
            r.unwrap();
        });
        Ok(())
    }

    fn search(&self, keywords: &str) -> Result<Vec<Comic>> {
        Ok(vec![])
    }

    fn paginated_search(&self, keywords: &str, page: u32) -> Result<Vec<Comic>> {
        self.search(keywords)
    }
}

pub struct ChapterPages<'a> {
    pub chapter: &'a mut Chapter,
    pub current_page: usize,
    fetch: Box<dyn Fn(usize) -> Result<Vec<Page>>>,
    pub total: i32,
}

impl<'a> ChapterPages<'a> {
    fn new(
        chapter: &'a mut Chapter,
        total: i32,
        init_addresses: Vec<String>,
        fetch: Box<dyn Fn(usize) -> Result<Vec<Page>>>,
    ) -> Self {
        for (i, address) in init_addresses.iter().enumerate() {
            chapter.pages.push(Page::new((i + 1) as usize, address));
        }
        ChapterPages {
            chapter,
            current_page: 0,
            fetch,
            total,
        }
    }

    fn full(chapter: &'a mut Chapter, addresses: Vec<String>) -> Self {
        Self::new(
            chapter,
            addresses.len() as i32,
            addresses,
            Box::new(move |_| Ok(vec![])),
        )
    }

    #[allow(dead_code)]
    pub fn chapter_title_clone(&self) -> String {
        self.chapter.title.clone()
    }
}

impl<'a> Iterator for ChapterPages<'a> {
    type Item = Result<Page>;

    fn next(&mut self) -> Option<Self::Item> {
        self.current_page += 1;
        if self.total == 0 || (self.total > 0 && (self.total as usize) < self.current_page) {
            return None;
        }
        let page_index = self.current_page - 1;
        if ((self.chapter.pages.len() as i32) - 1) >= page_index as i32 {
            return Some(Ok(self.chapter.pages[page_index].clone()));
        }

        match (self.fetch)(self.current_page) {
            Ok(mut pages) => {
                let count = pages.len();
                self.chapter.pages.append(&mut pages);
                let current_len = self.chapter.pages.len();
                if count > 0 {
                    Some(Ok(self.chapter.pages[current_len - count].clone()))
                } else {
                    None
                }
            }
            Err(e) => Some(Err(e)),
        }
    }
}

use reqwest::blocking::{Client, Response};
use scraper::{element_ref::ElementRef, Html, Selector};

fn parse_selector(selector: &str) -> Result<Selector> {
    Ok(Selector::parse(selector)
        .map_err(|_e| err_msg(format!("The selector '{}' parsing failed", selector)))?)
}

use reqwest::header::USER_AGENT;

pub static DEFAULT_USER_AGENT: &'static str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/79.0.3945.130 Safari/537.36";

pub fn get<T: reqwest::IntoUrl>(url: T) -> Result<Response> {
    Ok(Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?
        .get(url)
        .header(USER_AGENT, DEFAULT_USER_AGENT)
        .send()?)
}

pub fn eval_value(code: &str) -> Result<JsValue> {
    let context = Context::new()?;
    Ok(context.eval(code)?)
}

pub fn eval_as<R>(code: &str) -> Result<R>
where
    R: std::convert::TryFrom<JsValue>,
    R::Error: std::convert::Into<quick_js::ValueError>,
{
    let context = Context::new()?;
    Ok(context.eval_as::<R>(code)?)
}

trait Decode {
    fn decode_text(&mut self, encoding: &'static Encoding) -> Result<String>;
}

impl Decode for reqwest::blocking::Response {
    fn decode_text(&mut self, encoding: &'static Encoding) -> Result<String> {
        let mut buf: Vec<u8> = vec![];
        self.copy_to(&mut buf)?;
        let (cow, _encoding_used, _had_errors) = encoding.decode(&buf);
        Ok(cow[..].to_string())
    }
}

fn encode_text<'a>(text: &'a str, encoding: &'static Encoding) -> Result<Cow<'a, [u8]>> {
    let (cow, _encoding_used, _had_errors) = encoding.encode(text);
    Ok(cow)
}

type JsObject = HashMap<String, JsValue>;

pub fn eval_as_obj(code: &str) -> Result<JsObject> {
    match eval_value(code)? {
        JsValue::Object(obj) => Ok(obj),
        _ => Err(err_msg("Not a JS Object")),
    }
}

macro_rules! def_js_helper {
    ( to_object: [$( {:name => $name:ident, :js_t => $js_t:path, :result_t => $result_t:ty} ),*] ) => {
        trait JsObjectGetAndAs {
            $(
                fn $name(&self, key: &str) -> Result<&$result_t>;
            )*
        }
        impl JsObjectGetAndAs for JsObject {
            $(
                fn $name(&self, key: &str) -> Result<&$result_t> {
                    let value = self
                                .get(key)
                                .ok_or(err_msg(format!("Object property not found: {}", key)))?;
                    match value {
                        $js_t(v) => Ok(v),
                        _ => Err(err_msg(format!("Object property `{}` is not of type `{}`", key, stringify!($js_t))))
                    }
                }
            )*
        }
    };
    ( to_value: [$( {:name => $name:ident, :js_t => $js_t:path, :result_t => $result_t:ty} ),*] ) => {
        trait JsValueAs {
            $(
                fn $name(&self) -> Result<&$result_t>;
            )*
        }
        impl JsValueAs for JsValue {
            $(
                fn $name(&self) -> Result<&$result_t> {
                    match self {
                        $js_t(v) => Ok(v),
                        _ => Err(err_msg(format!("Object property is not of type `{}`", stringify!($js_t))))
                    }
                }
            )*
        }
    };
}

def_js_helper!(to_object: [
    {:name => get_as_string, :js_t => JsValue::String, :result_t => String},
    {:name => get_as_bool, :js_t => JsValue::Bool, :result_t => bool},
    {:name => get_as_int, :js_t => JsValue::Int, :result_t => i32},
    {:name => get_as_float, :js_t => JsValue::Float, :result_t => f64},
    {:name => get_as_array, :js_t => JsValue::Array, :result_t => Vec<JsValue>},
    {:name => get_as_object, :js_t => JsValue::Object, :result_t => JsObject}
]);

def_js_helper!(to_value: [
    {:name => as_string, :js_t => JsValue::String, :result_t => String},
    {:name => as_bool, :js_t => JsValue::Bool, :result_t => bool},
    {:name => as_int, :js_t => JsValue::Int, :result_t => i32},
    {:name => as_float, :js_t => JsValue::Float, :result_t => f64},
    {:name => as_array, :js_t => JsValue::Array, :result_t => Vec<JsValue>},
    {:name => as_object, :js_t => JsValue::Object, :result_t => JsObject}
]);

macro_rules! def_extractor {
    ( status => [$($name:ident: $value:expr),*], tags => [$($tn:ident),*], $($tt:tt)* ) => {
        pub struct Extr {
            status: Status,
            tags: Vec<Tag>,
        }
        impl Extractor for Extr {
            $($tt)*

            fn read_status(&self) -> &Status {
                &self.status
            }

            fn tags(&self) -> &Vec<Tag> {
                &self.tags
            }
        }
        impl Extr {
            fn new(status: Status, tags: Vec<Tag>) -> Self {
                Self { status, tags }
            }
        }
        pub fn new_extr() -> Extr {
            let mut status = Status::new();
            $(
                status.insert(stringify!($name), Box::new($value));
            )*
            let mut tags = vec![];
            $(
                tags.push(Tag::$tn);
            )*
            Extr::new(status, tags)
        }
    };
}

duang!(
    fn urlgen2(page : u32, first: &str = "", next: &str = "") -> String {
        if page > 1 {
            next.replace("{}", &page.to_string()).to_string()
        } else {
            first.to_string()
        }
    }
);

lazy_static! {
    pub static ref DEFAULT_STRING: String = "".to_string();
    pub static ref DEFAULT_REGEX: Regex = Regex::new("^_n_o_r_e_$").unwrap();
    pub static ref DEFAULT_FETCHING_FN: Box<dyn Fn(usize) -> Vec<String> + Sync + Send> =
        Box::new(|_| vec![]);
}

duang!(
    pub fn itemsgen2<T: FromLink + SetCover>(
        html: &str = "",
        url: &str =  "",
        encoding: &'static Encoding = UTF_8,
        target_dom: &str = "",
        target_text_dom: &str = "",
        target_text_attr: &str = "",
        parent_dom: &str = "",
        cover_dom: &str = "",
        cover_attr: &str = "src",
        cover_attrs: &[&str] = &[],
        cover_prefix: &str = "",
        link_dom: &str = "",
        link_attr: &str = "href",
        link_prefix: &str = "",
        link_text_attr: &str = "",
        link_text_dom: &str = "",
        ignore_contains: &str = ""
    ) -> Result<Vec<T>> {
        let html = if html.is_empty() {
            if url.is_empty() {
                panic!("Missing `url` parameter");
            }
            let mut resp = get(url)?;
            if encoding != UTF_8 {
                resp.decode_text(encoding)?
            } else {
                resp.text()?
            }.to_string()
        } else {
            html.to_string()
        };
        let document = parse_document(&html);
        let from_link = |element: &ElementRef| -> Result<T> {
            let mut url = element
                .value()
                .attr(link_attr)
                .ok_or(err_msg("No link href found"))?
                .to_string();
            if !link_prefix.is_empty() {
                url = format!("{}{}", link_prefix, url)
            }
            let mut title = String::new();
            if !target_text_dom.is_empty() {
                title = element.select(&parse_selector(target_text_dom)?)
                    .next()
                    .ok_or(err_msg(format!("No :target_text_dom node found: `{}`", target_text_dom)))?
                    .text()
                    .next()
                    .ok_or(err_msg(format!("No :target_text_dom text found: `{}`", target_text_dom)))?
                    .to_string();
            }
            if !link_text_dom.is_empty() {
                title = element.select(&parse_selector(link_text_dom)?)
                    .next()
                    .ok_or(err_msg(format!("No :link_text_dom node found: `{}`", link_text_dom)))?
                    .text()
                    .next()
                    .ok_or(err_msg(format!("No :link_text_dom text found: `{}`", link_text_dom)))?
                    .to_string();
            }
            if !target_text_attr.is_empty() {
                title = element.value()
                    .attr(target_text_attr)
                    .ok_or(err_msg(format!("No :target_text_attr found: `{}`", target_text_attr)))?
                    .to_string();
            }
            if !link_text_attr.is_empty() {
                title = element.value()
                    .attr(link_text_attr)
                    .ok_or(err_msg(format!("No attr found: `{}`", link_text_attr)))?
                    .to_string();
            }
            if title.is_empty() {
                title = element
                    .text()
                    .next()
                    .ok_or(err_msg("No link text found"))?
                    .to_string();
            }
            title = title.trim().to_string();
            Ok(T::from_link(title, url))
        };

        let mut items = vec![];
        if !parent_dom.is_empty() {
            let parent_dom_selecors = parse_selector(parent_dom)?;
            let parent_dom_select = document.select(&parent_dom_selecors);
            let parent_elems = if !ignore_contains.is_empty() {
                let ignore_selector = parse_selector(ignore_contains)?;
                parent_dom_select.filter(|select| {
                    select.select(&ignore_selector).next() == None
                }).collect::<Vec<_>>()
            } else {
                parent_dom_select.collect::<Vec<_>>()
            };
            for (i, parent_elem) in parent_elems.iter().enumerate() {
                let link_elem = if let Some(elem) = parent_elem.select(&parse_selector(link_dom)?).next() {
                    elem
                } else {
                    let e = err_msg(format!(
                        "Link node `{}` not found in index `{}`",
                        link_dom, i
                    ));
                    return Err(e);
                };
                let mut item = from_link(&link_elem)?;
                let cover_dom = parent_elem
                    .select(&parse_selector(cover_dom)?)
                    .next()
                    .ok_or(err_msg(format!("No cover DOM node found: `{}`", cover_dom)))?;
                let cover = if cover_attrs.len() > 0 {
                    let covers = cover_attrs.iter()
                        .map(|attr| {
                            cover_dom.value().attr(attr)
                        })
                        .filter(|cover| {
                            cover.is_some()
                        })
                        .collect::<Vec<_>>();
                    if covers.len() > 0 {
                        covers[0].unwrap()
                    } else {
                        return Err(err_msg(format!("No cover attrs found: `{}`", cover_attrs.join(","))))
                    }
                } else {
                    cover_dom
                        .value()
                        .attr(cover_attr)
                        .ok_or(err_msg(format!("No cover attr found: `{}`", cover_attr)))?
                };

                item.set_cover(format!("{}{}", cover_prefix, cover));
                items.push(item);
            }
        } else {
            if target_dom.is_empty() {
                panic!("Missing `target_dom` parameter");
            }
            let target_dom_selecors = parse_selector(target_dom)?;
            let target_dom_select = document.select(&target_dom_selecors);
            let link_elems = if !ignore_contains.is_empty() {
                let ignore_selector = parse_selector(ignore_contains)?;
                target_dom_select.filter(|select| {
                    select.select(&ignore_selector).next() == None
                }).collect::<Vec<_>>()
            } else {
                target_dom_select.collect::<Vec<_>>()
            };
            for link_elem in link_elems {
                items.push(from_link(&link_elem)?);
            }
        }

        Ok(items)
    }
);

trait AttachTo<T> {
    fn attach_to(self, target: &mut T);
    fn reversed_attach_to(self, target: &mut T);
    fn headers_clear(self) -> Self;
}

impl AttachTo<Comic> for Vec<Chapter> {
    fn attach_to(self, target: &mut Comic) {
        for (i, mut chapter) in self.into_iter().enumerate() {
            chapter.which = (i as u32) + 1;
            target.push_chapter(chapter);
        }
    }

    fn reversed_attach_to(mut self, target: &mut Comic) {
        self.reverse();
        self.attach_to(target);
    }

    fn headers_clear(mut self) -> Self {
        for chapter in &mut self {
            chapter.page_headers.clear();
        }
        self
    }
}

macro_rules! def_regex2 {
    ( $( $name:ident => $str:expr ),*, ) => {
        paste::item! {
            lazy_static! {
                $(
                    static ref [<$name _RE>]: Regex = Regex::new($str).unwrap();
                )*
            }
        }
    };
    ( $( $name:ident => $str:expr ),* ) => {
        def_regex2![ $($name => $str,)* ];
    }
}

duang!(
    fn match_content2(text: &str, regex: &Regex, group: usize = 1) -> Result<String> {
        let caps = regex.captures(text)
            .ok_or(err_msg(format!("No content was captured, regex: `{}`", regex)))?;

        let r = caps.get(group)
            .ok_or(err_msg(format!("No group found: {}, regex: `{}`", group, regex)))?
            .as_str()
            .to_string();

        Ok(r)
    }
);

macro_rules! wrap_code {
    ($code:expr, $custom:expr, :end) => {
        format!("{}\n{}", $code, $custom);
    };
}

#[test]
fn test_eval_as() {
    match eval_as::<String>("1 + 1") {
        Ok(_) => assert!(false),
        Err(_e) => assert!(true),
    }
    let result = eval_as::<String>("(1 + 1).toString()").unwrap();
    assert_eq!("2", result);
}

#[test]
fn test_eval_value() {
    let value = eval_value("1 + 1").unwrap();
    assert_eq!(JsValue::Int(2), value);
}

#[test]
fn test_eval_obj() {
    let code = r#"
        var obj = {
            a: 1,
            b: "b",
            c: {
                c1: true
            },
            d: ["d1"]
        };
        obj
    "#;
    let obj = eval_as_obj(&code).unwrap();
    assert_eq!(1, *obj.get_as_int("a").unwrap());
    assert_eq!(String::from("b"), *obj.get_as_string("b").unwrap());

    let c = obj.get_as_object("c").unwrap();
    assert_eq!(true, *c.get_as_bool("c1").unwrap());

    let d = obj.get_as_array("d").unwrap();
    assert_eq!(1, d.len());
    assert_eq!(String::from("d1"), *d[0].as_string().unwrap());
}

type ExtractorObject = Box<dyn Extractor + Sync + Send>;

macro_rules! import_impl_mods {
    ( $($module:ident: {:domain => $domain:expr, :name => $name:expr}),* ) => {
        $(
            #[cfg(feature = $domain)]
            pub mod $module;
        )*
        lazy_static!{
            pub static ref PLATFORMS: HashMap<String, String> = {
                let mut platforms = HashMap::new();
                $(
                    #[cfg(feature = $domain)]
                    platforms.insert($domain.to_string(), $name.to_string());
                )*
                platforms
            };

            pub static ref EXTRACTORS: HashMap<String, ExtractorObject> = {
                let mut extractros: HashMap<String, ExtractorObject> = HashMap::new();
                $(
                    #[cfg(feature = $domain)]
                    extractros.insert($domain.to_string(), Box::new($module::new_extr()));
                )*
                extractros
            };
        }
    }
}

import_impl_mods![
    bidongmh: {
        :domain => "www.bidongmh.com",
        :name   => "壁咚漫画"
    },
    bnmanhua: {
        :domain => "www.bnmanhua.com",
        :name   => "百年漫画"
    },
    cartoonmad: {
        :domain => "www.cartoonmad.com",
        :name   => "動漫狂"
    },
    comico: {
        :domain => "www.comico.com.tw",
        :name   => "comico"
    },
    dm5: {
        :domain => "www.dm5.com",
        :name   => "动漫屋（漫画人）"
    },
    dmjz: {
        :domain => "manhua.dmzj.com",
        :name   => "动漫之家"
    },
    ehentai: {
        :domain => "e-hentai.org",
        :name   => "E-Hentai"
    },
    eighteenh: {
        :domain => "18h.animezilla.com",
        :name   => "18H 宅宅愛動漫"
    },
    gufengmh8: {
        :domain => "www.gufengmh8.com",
        :name   => "古风漫画网"
    },
    hcomic: {
        :domain => "c-upp.com",
        :name   => "喵绅士"
    },
    hhimm: {
        :domain => "www.hhimm.com",
        :name   => "汗汗酷漫"
    },
    ikkdm: {
        :domain => "comic.ikkdm.com",
        :name   => "KuKu动漫"
    },
    kuaikanmanhua: {
        :domain => "www.kuaikanmanhua.com",
        :name   => "快看漫画"
    },
    loveheaven: {
        :domain => "loveheaven.net",
        :name   => "LoveHeaven"
    },
    luscious: {
        :domain => "www.luscious.net",
        :name   => "Luscious"
    },
    mangabz: {
        :domain => "www.mangabz.com",
        :name   => "Mangabz"
    },
    manganelo: {
        :domain => "manganelo.com",
        :name   => "Manganelo"
    },
    mangareader: {
        :domain => "www.mangareader.net",
        :name   => "Mangareader"
    },
    manhuadui: {
        :domain => "www.manhuadui.com",
        :name   => "漫画堆"
    },
    manhuadb: {
        :domain => "www.manhuadb.com",
        :name   => "漫画DB"
    },
    manhuagui: {
        :domain => "www.manhuagui.com",
        :name   => "漫画柜"
    },
    manhuapu: {
        :domain => "www.manhuapu.com",
        :name   => "漫画铺"
    },
    mkzhan: {
        :domain => "www.mkzhan.com",
        :name   => "漫客栈"
    },
    nhentai: {
        :domain => "nhentai.net",
        :name   => "nhentai"
    },
    ninehentai: {
        :domain => "9hentai.com",
        :name   => "9hentai"
    },
    ninetymh: {
        :domain => "www.90mh.com",
        :name   => "90漫画网"
    },
    one77pic: {
        :domain => "www.177pic.info",
        :name   => "177漫畫"
    },
    onemanhua: {
        :domain => "www.onemanhua.com",
        :name   => "ONE漫画"
    },
    pufei8: {
        :domain => "www.pufei8.com",
        :name   => "扑飞漫画"
    },
    qimiaomh: {
        :domain => "www.qimiaomh.com",
        :name   => "奇妙漫画"
    },
    tohomh123: {
        :domain => "www.tohomh123.com",
        :name   => "土豪漫画"
    },
    tvbsmh: {
        :domain => "www.tvbsmh.com",
        :name   => "TVBS漫畫"
    },
    twhentai: {
        :domain => "twhentai.com",
        :name   => "台灣成人H漫"
    },
    twoanimx: {
        :domain => "www.2animx.com",
        :name   => "二次元動漫"
    },
    wnacg: {
        :domain => "www.wnacg.org",
        :name   => "紳士漫畫"
    },
    wuqimh: {
        :domain => "www.wuqimh.com",
        :name   => "57漫画网"
    },
    xinxinmh: {
        :domain => "www.177mh.net",
        :name   => "新新漫画网"
    },
    yylsmh: {
        :domain => "8comic.se",
        :name   => "YYLS漫畫"
    }
];

pub fn platforms() -> &'static HashMap<String, String> {
    &*PLATFORMS
}

pub fn find_platforms(includes: Vec<Tag>, excludes: Vec<Tag>) -> HashMap<String, String> {
    (&*PLATFORMS)
        .iter()
        .filter(|(domain, _)| {
            if let Some(extr) = get_extr(*domain) {
                // 包含标签
                if includes.len() > 0 {
                    extr.tags()
                        .iter()
                        .filter(|tag: &&Tag| includes.contains(*tag))
                        .collect::<Vec<_>>()
                        .len()
                        > 0
                } else {
                    true
                }
            } else {
                false
            }
        })
        .filter(|(domain, _)| {
            if let Some(extr) = get_extr(*domain) {
                // 排除标签
                extr.tags()
                    .iter()
                    .filter(|tag: &&Tag| excludes.contains(*tag))
                    .collect::<Vec<_>>()
                    .len()
                    == 0
            } else {
                false
            }
        })
        .map(|(domain, name)| (domain.to_string(), name.to_string()))
        .collect::<HashMap<String, String>>()
}

#[test]
fn test_find_platforms() {
    let platforms = find_platforms(vec![Tag::Chinese], vec![]);
    assert_eq!(
        platforms.get("www.wnacg.org"),
        Some(&String::from("紳士漫畫"))
    );
    let platforms = find_platforms(vec![Tag::Chinese], vec![Tag::NSFW]);
    assert_eq!(platforms.get("www.wnacg.org"), None);
}

pub fn get_extr<S: Into<String>>(domain: S) -> Option<&'static ExtractorObject> {
    EXTRACTORS.get(&domain.into())
}

type Routes = Vec<(String, (Regex, Regex))>;

macro_rules! def_routes {
    ( $({:domain => $domain:expr, :comic_re => $comic_re:expr, :chapter_re => $chapter_re:expr}),* ) => {
        lazy_static!{
            static ref ROUTES: Routes = {
                let mut routes: Routes = Vec::new();
                $(
                    routes.push(($domain.to_string(), (Regex::new($comic_re).unwrap(), Regex::new($chapter_re).unwrap())));
                )*
                routes
            };
        }
    };
}

#[derive(Debug, PartialEq)]
pub enum DomainRoute {
    Comic(String),
    Chapter(String),
}

pub fn domain_route(url: &str) -> Option<DomainRoute> {
    for (domain, (comic_re, chapter_re)) in &*ROUTES {
        if chapter_re.is_match(url) {
            return Some(DomainRoute::Chapter(domain.clone()));
        }
        if comic_re.is_match(url) {
            return Some(DomainRoute::Comic(domain.clone()));
        }
    }
    None
}

def_routes![
    {
        :domain     => "www.bidongmh.com",
        :comic_re   => r#"^https?://www\.bidongmh\.com/book/\d+"#,
        :chapter_re => r#"^https?://www\.bidongmh\.com/chapter/\d+"#
    },
    {
        :domain     => "www.bnmanhua.com",
        :comic_re   => r#"^https?://www\.bnmanhua\.com/comic/\d+\.html"#,
        :chapter_re => r#"^https?://www\.bnmanhua\.com/comic/\d+/\d+\.html"#
    },
    {
        :domain     => "www.cartoonmad.com",
        :comic_re   => r#"^https?://www\.cartoonmad\.com/comic/\d{1,5}\.html"#,
        :chapter_re => r#"^https?://www\.cartoonmad\.com/comic/\d{11,}.html"#
    },
    {
        :domain     => "www.comico.com.tw",
        :comic_re   => r#"^https?://www\.comico\.com\.tw/challenge/\d+"#,
        :chapter_re => r#"^https?://www\.comico\.com\.tw/challenge/\d+/\d+/"#
    },
    {
        :domain     => "www.dm5.com",
        :comic_re   => r#"^https?://www\.dm5\.com/[^/]+/"#,
        :chapter_re => r#"^https?://www\.dm5\.com/m\d+/"#
    },
    {
        :domain     => "manhua.dmzj.com",
        :comic_re   => r#"^https?://manhua\.dmzj\.com/[^/]+/"#,
        :chapter_re => r#"^https?://manhua\.dmzj\.com/[^/]+/\d+\.shtml"#
    },
    {
        :domain     => "e-hentai.org",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://e-hentai\.org/g/\d+/[^/]+/"#
    },
    {
        :domain     => "18h.animezilla.com",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://18h\.animezilla\.com/manga/\d+"#
    },
    {
        :domain     => "www.gufengmh8.com",
        :comic_re   => r#"^https?://www\.gufengmh8\.com/manhua/.+"#,
        :chapter_re => r#"^https?://www\.gufengmh8\.com/manhua/[^/]+/\d+\.html"#
    },
    {
        :domain     => "c-upp.com",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://c-upp\.com/ja/s/\d+"#
    },
    {
        :domain     => "www.hhimm.com",
        :comic_re   => r#"^https?://www\.hhimm\.com/manhua/\d+\.html"#,
        :chapter_re => r#"^https?://www\.hhimm\.com/cool\d+/\d+\.html"#
    },
    {
        :domain     => "www.pufei8.com",
        :comic_re   => r#"^https?://www\.pufei8\.com/manhua/\d+/index\.html"#,
        :chapter_re => r#"^https?://www\.pufei8\.com/manhua/\d+/\d+\.html"#
    },
    {
        :domain     => "www.kuaikanmanhua.com",
        :comic_re   => r#"^https?://www\.kuaikanmanhua\.com/web/topic/\d+"#,
        :chapter_re => r#"^https?://www\.kuaikanmanhua\.com/web/comic/\d+"#
    },
    {
        :domain     => "comic.ikkdm.com",
        :comic_re   => r#"^https?://comic\.ikkdm\.com/comiclist/\d+/index.htm"#,
        :chapter_re => r#"^https?://comic\d?\.ikkdm\.com/comiclist/\d+/\d+/\d+.htm"#
    },
    {
        :domain     => "loveheaven.net",
        :comic_re   => r#"^https?://loveheaven\.net/manga-.+\.html"#,
        :chapter_re => r#"^https?://loveheaven\.net/read-.+\.html"#
    },
    {
        :domain     => "www.luscious.net",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://www\.luscious\.net/albums/.+"#
    },
    {
        :domain     => "www.mangabz.com",
        :comic_re   => r#"^https?://www\.mangabz\.com/.+"#,
        :chapter_re => r#"^https?://www\.mangabz\.com/m\d+"#
    },
    {
        :domain     => "manganelo.com",
        :comic_re   => r#"^https?://manganelo\.com/manga/.+"#,
        :chapter_re => r#"^https?://manganelo\.com/chapter/[^/]+/chapter_.+"#
    },
    {
        :domain     => "www.manhuadb.com",
        :comic_re   => r#"^https?://www\.manhuadb\.com/manhua/.+"#,
        :chapter_re => r#"^https?://www\.manhuadb\.com/manhua/\d+/\d+_\d+\.html"#
    },
    {
        :domain     => "www.manhuadui.com",
        :comic_re   => r#"^https?://www\.manhuadui\.com/manhua/.+"#,
        :chapter_re => r#"^https?://www\.manhuadui\.com/manhua/[^/]+/\d+\.html"#
    },
    {
        :domain     => "www.manhuagui.com",
        :comic_re   => r#"^https?://www\.manhuagui\.com/comic/\d+/"#,
        :chapter_re => r#"^https?://www\.manhuagui\.com/comic/\d+/\d+\.html"#
    },
    {
        :domain     => "www.manhuapu.com",
        :comic_re   => r#"^https?://www\.manhuapu\.com/[^/]+/.+"#,
        :chapter_re => r#"^https?://www\.manhuapu\.com/[^/]+/[^/]+/\d+\.html"#
    },
    {
        :domain     => "nhentai.net",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://nhentai\.net/g/\d+"#
    },
    {
        :domain     => "9hentai.com",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://9hentai\.com/g/\d+"#
    },
    {
        :domain     => "www.90mh.com",
        :comic_re   => r#"^https?://www\.90mh\.com/manhua/.+"#,
        :chapter_re => r#"^https?://www\.90mh\.com/manhua/[^/]+/\d+\.html"#
    },
    {
        :domain     => "www.177pic.info",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://www\.177pic\.info/html/\d+/\d+/\d+\.html"#
    },
    {
        :domain     => "www.onemanhua.com",
        :comic_re   => r#"^https?://www\.onemanhua\.com/\d+"#,
        :chapter_re => r#"^https?://www\.onemanhua\.com/\d+/\d+/\d+\.html"#
    },
    {
        :domain     => "www.qimiaomh.com",
        :comic_re   => r#"^https?://www\.qimiaomh\.com/manhua/\d+\.html"#,
        :chapter_re => r#"^https?://www\.qimiaomh\.com/manhua/\d+/\d+\.html"#
    },
    {
        :domain     => "www.tohomh123.com",
        :comic_re   => r#"^https?://www\.tohomh123\.com/.+"#,
        :chapter_re => r#"^https?://www\.tohomh123\.com/[^/]+/\d+\.html"#
    },
    {
        :domain     => "twhentai.com",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://twhentai\.com/[^/]+/\d+"#
    },
    {
        :domain     => "www.2animx.com",
        :comic_re   => r#"^https?://www\.2animx\.com/index-comic-name-.+-id-\d+"#,
        :chapter_re => r#"^https?://www\.2animx\.com/index-look-name-.+-cid-\d+-id-\d+"#
    },
    {
        :domain     => "www.wnacg.org",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://www\.wnacg\.org/photos-index-(page-\d+-)?aid-\d+\.html"#
    },
    {
        :domain     => "www.wuqimh.com",
        :comic_re   => r#"^https?://www\.wuqimh\.com/\d+"#,
        :chapter_re => r#"^https?://www\.wuqimh\.com/\d+/\d+\.html"#
    },
    {
        :domain     => "www.177mh.net",
        :comic_re   => r#"^https?://www\.177mh\.net/colist_\d+\.html"#,
        :chapter_re => r#"^https?://www.177mh.net/\d+/\d+\.html"#
    },
    {
        :domain     => "8comic.se",
        :comic_re   => r#"^-NONE-$"#,
        :chapter_re => r#"^https?://8comic\.se/\d+"#
    }
];

#[test]
fn test_favicons() {
    let favicon = get_extr("8comic.se").unwrap().get_favicon().unwrap();
    assert_eq!("https://8comic.se/favicon.ico", *favicon);
}
