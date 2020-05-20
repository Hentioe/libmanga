use super::*;
use serde::Deserialize;
use serde_json::Value;

def_regex2![
    ID      => r#"https?://www\.luscious\.net/albums/[^/]+_(\d+)/"#
];

#[derive(Debug, Deserialize)]
struct Cover {
    url: String,
}

#[derive(Debug, Deserialize)]
struct ComicItem {
    title: String,
    url: String,
    cover: Cover,
}

impl From<&ComicItem> for Comic {
    fn from(c: &ComicItem) -> Self {
        Self::from_index(
            &c.title,
            &format!("https://www.luscious.net{}", &c.url),
            &c.cover.url,
        )
    }
}

// 对 www.luscious.net 内容的抓取实现
def_extractor! {
    status	=> [
        usable: true, pageable: true, searchable: true, https: true, pageable_search: true,
        favicon: "https://www.luscious.net/favicon.ico"
    ],
    tags	=> [English, Japanese, Chinese, NSFW],

    fn index(&self, page: u32) -> Result<Vec<Comic>> {
        let mut url = String::from(r#"https://api.luscious.net/graphql/nobatch/?operationName=AlbumList&query=+query+AlbumList($input:+AlbumListInput!)+{+album+{+list(input:+$input)+{+info+{+...FacetCollectionInfo+}+items+{+...AlbumMinimal+}+}+}+}+fragment+FacetCollectionInfo+on+FacetCollectionInfo+{+page+has_next_page+has_previous_page+total_items+total_pages+items_per_page+url_complete+}+fragment+AlbumMinimal+on+Album+{+__typename+id+title+labels+description+created+modified+like_status+status+number_of_favorites+number_of_dislikes+number_of_pictures+number_of_animated_pictures+number_of_duplicates+slug+is_manga+url+download_url+permissions+created_by+{+id+url+name+display_name+user_title+avatar+{+url+size+}+}+cover+{+width+height+size+url+}+content+{+id+title+url+}+language+{+id+title+url+}+tags+{+id+category+text+url+count+}+genres+{+id+title+slug+url+}+audiences+{+id+title+url+}+}+&variables={"input":{"display":"date_trending","filters":[{"name":"album_type","value":"manga"}],"page":"#);
        url.push_str(&page.to_string());
        url.push_str("}}");
        let json_v = get(&url)?.json::<Value>()?;
        let list = json_v["data"]["album"]["list"]["items"].clone();
        let comics = serde_json::from_value::<Vec<ComicItem>>(list)?
            .iter()
            .map(|c: &ComicItem| { Comic::from(c) })
            .collect::<Vec<_>>();

        Ok(comics)
    }

    fn paginated_search(&self, keywords: &str, page: u32) -> Result<Vec<Comic>> {
        let mut url = String::from(r#"https://api.luscious.net/graphql/nobatch/?operationName=AlbumList&query=+query+AlbumList($input:+AlbumListInput!)+{+album+{+list(input:+$input)+{+info+{+...FacetCollectionInfo+}+items+{+...AlbumMinimal+}+}+}+}+fragment+FacetCollectionInfo+on+FacetCollectionInfo+{+page+has_next_page+has_previous_page+total_items+total_pages+items_per_page+url_complete+}+fragment+AlbumMinimal+on+Album+{+__typename+id+title+labels+description+created+modified+like_status+moderation_status+number_of_favorites+number_of_dislikes+number_of_pictures+number_of_animated_pictures+number_of_duplicates+slug+is_manga+url+download_url+permissions+created_by+{+id+url+name+display_name+user_title+avatar+{+url+size+}+}+cover+{+width+height+size+url+}+content+{+id+title+url+}+language+{+id+title+url+}+tags+{+category+text+url+count+}+genres+{+id+title+slug+url+}+audiences+{+id+title+url+}+}+&variables={"input":{"display":"search_score","filters":[{"name":"album_type","value":"manga"},{"name":"audience_ids","value":"+1+10+2+3+5+6+8+9"},{"name":"language_ids","value":"+1+100+101+2+3+4+5+6+8+9+99"},{"name":"search_query","value":""#);
        url.push_str(keywords);
        url.push_str(r#""}],"page":"#);
        url.push_str(&page.to_string());
        url.push_str(r#"}}"#);
        let json_v = get(&url)?.json::<Value>()?;
        let items = json_v["data"]["album"]["list"]["items"].clone();
        if matches!(items, Value::Null) { // 空结果
            return Ok(vec![]);
        }

        let comics = serde_json::from_value::<Vec<ComicItem>>(items)?
            .iter()
            .map(|c: &ComicItem| { Comic::from(c) })
            .collect::<Vec<_>>();

        Ok(comics)
    }

    fn fetch_chapters(&self, comic: &mut Comic) -> Result<()> {
        comic.push_chapter(Chapter::from(&*comic));

        Ok(())
    }

    fn pages_iter<'a>(&'a self, chapter: &'a mut Chapter) -> Result<ChapterPages> {
        let id = &match_content2!(&chapter.url, &*ID_RE)?;
        // 获取漫画信息
        let mut get_api = String::from(r#"https://api.luscious.net/graphql/nobatch/?operationName=AlbumGet&query=+query+AlbumGet($id:+ID!)+{+album+{+get(id:+$id)+{+...+on+Album+{+...AlbumStandard+}+...+on+MutationError+{+errors+{+code+message+}+}+}+}+}+fragment+AlbumStandard+on+Album+{+__typename+id+title+labels+description+created+modified+like_status+number_of_favorites+number_of_dislikes+rating+status+marked_for_deletion+marked_for_processing+number_of_pictures+number_of_animated_pictures+number_of_duplicates+slug+is_manga+url+download_url+permissions+cover+{+width+height+size+url+}+created_by+{+id+url+name+display_name+user_title+avatar+{+url+size+}+}+content+{+id+title+url+}+language+{+id+title+url+}+tags+{+category+text+url+count+}+genres+{+id+title+slug+url+}+audiences+{+id+title+url+url+}+last_viewed_picture+{+id+position+url+}+is_featured+featured_date+featured_by+{+id+url+name+display_name+user_title+avatar+{+url+size+}+}+}+&variables={"id":""#);
        get_api.push_str(id);
        get_api.push_str(r#""}"#);
        let json_v = get(&get_api)?.json::<Value>()?;
        let title = json_v["data"]["album"]["get"]["title"].as_str().ok_or(err_msg("No title found"))?;
        chapter.set_title(title);

        // 获取图片资源
        let mut pictures_api = String::from(r#"https://api.luscious.net/graphql/nobatch/?operationName=AlbumListOwnPictures&query=+query+AlbumListOwnPictures($input:+PictureListInput!)+{+picture+{+list(input:+$input)+{+info+{+...FacetCollectionInfo+}+items+{+...PictureStandardWithoutAlbum+}+}+}+}+fragment+FacetCollectionInfo+on+FacetCollectionInfo+{+page+has_next_page+has_previous_page+total_items+total_pages+items_per_page+url_complete+}+fragment+PictureStandardWithoutAlbum+on+Picture+{+__typename+id+title+created+like_status+number_of_comments+number_of_favorites+status+width+height+resolution+aspect_ratio+url_to_original+url_to_video+is_animated+position+tags+{+category+text+url+}+permissions+url+thumbnails+{+width+height+size+url+}+}+&variables={"input":{"filters":[{"name":"album_id","value":""#);
        pictures_api.push_str(id);
        pictures_api.push_str(r#""}],"display":"position","page":1}}"#);
        let json_v = get(&pictures_api)?.json::<Value>()?;
        let items = json_v["data"]["picture"]["list"]["items"].as_array().ok_or(err_msg("No pictures found"))?;

        let mut addresses = vec![];
        for item in items {
            let mut address = item["url_to_original"].as_str().ok_or(err_msg("No picture url found"))?.to_owned();
            if address.starts_with("//") {
                address = format!("https:{}", address);
            }
            addresses.push(address);
        }

        Ok(ChapterPages::full(chapter, addresses))
    }
}

#[test]
fn test_extr() {
    let extr = new_extr();
    let comics = extr.index(1).unwrap();
    assert_eq!(30, comics.len());

    let comic1 = Comic::from_link(
        "Teitoku wa Semai Toko Suki (Kantai Collection -KanColle-) [English]", 
        "https://www.luscious.net/albums/teitoku-wa-semai-toko-suki-kantai-collection-kanco_363520/"
    );
    let chapter1 = &mut Chapter::from(&comic1);
    extr.fetch_pages_unsafe(chapter1).unwrap();
    assert_eq!(
        "Teitoku wa Semai Toko Suki (Kantai Collection -KanColle-) [English]",
        chapter1.title
    );
    assert_eq!(26, chapter1.pages.len());

    let chapter2 = &mut Chapter::from_url(
        "https://www.luscious.net/albums/oreimo-selection-2016-fuyu_371269/",
    );
    extr.fetch_pages_unsafe(chapter2).unwrap();
    assert_eq!("Oreimo Selection 2016 Fuyu", chapter2.title);
    assert_eq!(9, chapter2.pages.len());

    let chapter3 = &mut Chapter::from_url(
        "https://www.luscious.net/albums/koibashira_no_oneesan_to_issh_353457/",
    );
    extr.fetch_pages_unsafe(chapter3).unwrap();
    assert_eq!(
        "Koibashira no Onee-san to Isshoni Shugyou Shiyou + SP",
        chapter3.title
    );
    assert_eq!(22, chapter3.pages.len());

    let comics = extr
        .paginated_search("Teitoku wa Semai Toko Suki", 1)
        .unwrap();
    assert!(comics.len() > 0);
    assert_eq!(comics[0].title, comic1.title);
    assert_eq!(comics[0].url, comic1.url);
}
