use std::time::Duration;

#[cfg(feature = "tokio")]
#[inline]
pub async fn sleep(dur: Duration) {
    tokio::time::sleep(dur).await
}

// #[cfg(feature = "compio")]
// #[inline]
// pub async fn sleep(dur: Duration) {
//     compio::runtime::time::sleep(dur).await
// }

pub fn get_query_string_element<'a>(qry: &'a str, key: &'static str) -> Option<&'a str> {
    for el in qry.split("&") {
        let Some(eq) = el.find("=") else {
            continue;
        };
        let got_key = &el[..eq];
        if got_key != key {
            continue;
        }
        let val = &el[key.len() + 1..];
        return Some(val);
    }
    None
}

#[test]
fn test_get_query_string_element() {
    let qry = "a=b&cde=d";
    assert!(get_query_string_element(&qry, "cd").is_none());
    assert!(get_query_string_element(&qry, "a").is_some());
    let res = get_query_string_element(&qry, "cde");
    assert!(res.is_some());
    let res = res.unwrap();
    assert_eq!(res, "d");
}
