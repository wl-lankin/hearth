use crate::mail::store::{self, Envelope};
use crate::StokePaths;

fn home() -> (tempfile::TempDir, StokePaths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = StokePaths::at(dir.path().join("stoke"));
    paths.ensure().unwrap();
    (dir, paths)
}

fn envelope(site: Option<&str>) -> Envelope {
    Envelope { site: site.map(String::from), from: "app@example.test".into(), to: vec!["ann@example.test".into()] }
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfake";

fn multipart() -> Vec<u8> {
    use base64::Engine;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    format!(
        "From: Shop <shop@example.test>\r\nTo: Ann <ann@example.test>\r\nCc: bob@example.test\r\nSubject: Your order\r\n\
MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n\
--outer\r\nContent-Type: multipart/related; boundary=\"rel\"\r\n\r\n\
--rel\r\nContent-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n\
--alt\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nThanks for your order.\r\n\
--alt\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Thanks <img src=\"cid:logo@shop\"></p>\r\n\
--alt--\r\n\
--rel\r\nContent-Type: image/png\r\nContent-ID: <logo@shop>\r\nContent-Disposition: inline\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n\
--rel--\r\n\
--outer\r\nContent-Type: application/pdf; name=\"invoice.pdf\"\r\nContent-Disposition: attachment; filename=\"invoice.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n\
--outer--\r\n",
        b64(PNG),
        b64(b"%PDF-1.4 fake")
    )
    .into_bytes()
}

#[test]
fn save_then_list_and_get_a_multipart_mail() {
    let (_d, paths) = home();
    let saved = store::save(&paths, &envelope(Some("shop")), &multipart()).unwrap();
    assert_eq!(saved.subject, "Your order");
    assert_eq!(saved.from, "Shop <shop@example.test>");
    assert_eq!(saved.to, vec!["ann@example.test".to_string()]);
    assert_eq!(saved.site.as_deref(), Some("shop"));
    assert_eq!(saved.attachments, 1);
    assert!(!saved.seen);
    assert_eq!(store::list(&paths), vec![saved.clone()]);

    let mail = store::get(&paths, &saved.id).unwrap();
    assert_eq!(mail.cc, vec!["bob@example.test".to_string()]);
    assert_eq!(mail.text.as_deref().map(str::trim), Some("Thanks for your order."));
    assert!(mail.html.as_deref().unwrap().starts_with("<p>Thanks"));
    assert_eq!(mail.files.len(), 1);
    let a = &mail.files[0];
    assert_eq!((a.index, a.name.as_str(), a.content_type.as_str(), a.size), (0, "invoice.pdf", "application/pdf", 13));
    assert_eq!(
        store::attachment(&paths, &saved.id, 0).unwrap(),
        ("invoice.pdf".to_string(), b"%PDF-1.4 fake".to_vec())
    );
    assert!(store::attachment(&paths, &saved.id, 1).is_err());
    assert!(store::list(&paths)[0].seen, "opening a mail marks it read");
}

#[test]
fn inline_cid_images_become_data_urls() {
    use base64::Engine;
    let (_d, paths) = home();
    let saved = store::save(&paths, &envelope(None), &multipart()).unwrap();
    let html = store::get(&paths, &saved.id).unwrap().html.unwrap();
    let data = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(PNG));
    assert!(html.contains(&format!("src=\"{data}\"")), "{html}");
    assert!(!html.contains("cid:"), "{html}");
}

#[test]
fn a_broken_mail_is_still_kept() {
    let (_d, paths) = home();
    let raw = b"just some text\nwith \xe4 8-bit bytes\n\nand no headers\n".to_vec();
    let saved = store::save(&paths, &envelope(None), &raw).unwrap();
    assert_eq!(saved.subject, "(no subject)");
    assert_eq!(saved.from, "app@example.test", "the envelope sender stands in");
    let mail = store::get(&paths, &saved.id).unwrap();
    assert!(mail.text.is_some() || mail.html.is_some());
    let (text, truncated) = store::raw(&paths, &saved.id).unwrap();
    assert_eq!(text, String::from_utf8_lossy(&raw));
    assert!(!truncated);
    assert_eq!(std::fs::read(paths.data.join("mail").join(format!("{}.eml", saved.id))).unwrap(), raw);
    let empty = store::save(&paths, &envelope(None), b"").unwrap();
    assert_eq!(store::get(&paths, &empty.id).unwrap().text, None);
}

#[test]
fn raw_is_cut_at_one_megabyte() {
    let (_d, paths) = home();
    let mut raw = b"Subject: big\r\n\r\n".to_vec();
    raw.resize(1024 * 1024 + 10, b'x');
    let saved = store::save(&paths, &envelope(None), &raw).unwrap();
    let (text, truncated) = store::raw(&paths, &saved.id).unwrap();
    assert_eq!((text.len(), truncated), (1024 * 1024, true));
}

#[test]
fn ids_sort_by_arrival_and_only_the_newest_are_kept() {
    let (_d, paths) = home();
    let ids: Vec<String> = (0..store::KEEP + 2)
        .map(|i| store::save(&paths, &envelope(None), format!("Subject: {i}\r\n\r\nx").as_bytes()).unwrap().id)
        .collect();
    let listed = store::list(&paths);
    assert_eq!(listed.len(), store::KEEP);
    assert_eq!(listed[0].id, ids[ids.len() - 1], "newest first");
    assert_eq!(listed[listed.len() - 1].id, ids[2], "the two oldest are gone");
    assert!(store::get(&paths, &ids[0]).is_err());
}

#[test]
fn clear_by_site_and_unnamed() {
    let (_d, paths) = home();
    let one = |site| store::save(&paths, &envelope(site), b"Subject: x\r\n\r\ny").unwrap();
    let (a, b, c) = (one(Some("a")), one(Some("b")), one(None));
    store::clear(&paths, Some(None)).unwrap();
    let ids = |p: &StokePaths| store::list(p).into_iter().map(|s| s.id).collect::<Vec<_>>();
    assert_eq!(ids(&paths), vec![b.id.clone(), a.id.clone()]);
    store::clear(&paths, Some(Some("a"))).unwrap();
    assert_eq!(ids(&paths), vec![b.id.clone()]);
    store::delete(&paths, &b.id).unwrap();
    assert!(ids(&paths).is_empty());
    let _ = (c, one(Some("z")));
    store::clear(&paths, None).unwrap();
    assert!(ids(&paths).is_empty());
    assert!(std::fs::read_dir(paths.data.join("mail")).unwrap().next().is_none(), "no files left");
}

#[test]
fn ids_from_outside_are_checked() {
    let (_d, paths) = home();
    std::fs::write(paths.data.join("x.eml"), b"Subject: secret\r\n\r\n").unwrap();
    for id in ["../x", "..\\x", "", "1234567890123-0001/../../x", "abc"] {
        assert!(store::get(&paths, id).is_err(), "{id}");
        assert!(store::raw(&paths, id).is_err(), "{id}");
        assert!(store::delete(&paths, id).is_err(), "{id}");
    }
}
