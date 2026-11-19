mod common;

use gateterm_server::config::SharedWebTitle;

#[tokio::test]
async fn default_index_html_keeps_gateterm_title() {
    let addr = common::start_test_server_with_web_title(SharedWebTitle::default()).await;
    let res = reqwest::get(format!("http://{addr}/")).await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "text/html; charset=utf-8");
    assert_eq!(res.headers()["cache-control"], "no-cache");
    let body = res.text().await.unwrap();
    assert!(body.contains("<title>gateterm</title>"), "{body}");
    assert!(body.contains(r#"<span id="logo">gateterm</span>"#), "{body}");
}

#[tokio::test]
async fn custom_web_title_applies_to_root_index_and_spa_fallback_and_updates_live() {
    let title = SharedWebTitle::new(Some("prod-01".into()));
    let addr = common::start_test_server_with_web_title(title.clone()).await;

    for path in ["/", "/index.html", "/sessions/1"] {
        let res = reqwest::get(format!("http://{addr}{path}")).await.unwrap();
        assert_eq!(res.status(), 200);
        assert_eq!(res.headers()["content-type"], "text/html; charset=utf-8");
        assert_eq!(res.headers()["cache-control"], "no-cache");
        let body = res.text().await.unwrap();
        assert!(body.contains("<title>prod-01 - gateterm</title>"), "path={path}: {body}");
        assert!(body.contains(r#"<span id="logo">gateterm</span>"#), "path={path}: {body}");
    }

    title.set(Some(r#"dev <2> & "ok""#.into()));
    let body = reqwest::get(format!("http://{addr}/")).await.unwrap().text().await.unwrap();
    assert!(
        body.contains("<title>dev &lt;2&gt; &amp; &quot;ok&quot; - gateterm</title>"),
        "{body}"
    );

    title.set(Some("   ".into()));
    let body = reqwest::get(format!("http://{addr}/")).await.unwrap().text().await.unwrap();
    assert!(body.contains("<title>gateterm</title>"), "{body}");
}
