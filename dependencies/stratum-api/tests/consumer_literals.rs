//! The public structs a consumer builds field by field take `..Default::default()`, so a field added
//! to the layer (as `image` was to `Price`) does not break the consumer's build.
use stratum_api::{Ask, Inner, caps::Cap, chat::Route, prices::Price};

#[test]
fn consumer_literals_take_defaults() {
    let p = Price {
        key: "openai:m".into(),
        model: "m".into(),
        input: 2000,
        cached_input: 100,
        output: 10000,
        ..Default::default()
    };
    assert_eq!(p.image, None);
    assert_eq!(p.long, None);
    assert_eq!(
        stratum_api::prices::reserve(&p, 10, 1),
        Some(10 * 2000 + 10000)
    );
    let a = Ask {
        tool: "qroc",
        provider: "openai",
        model: "m",
        ..Default::default()
    };
    assert!(a.inner.is_none() && a.id.is_none());
    let i = Inner::default();
    assert!(i.state.as_os_str().is_empty());
    let r = Route {
        label: "openai:m".into(),
        max_output: 10,
        ..Default::default()
    };
    assert_eq!(r.effort, None);
    assert_eq!(
        Cap {
            total: 1,
            ..Default::default()
        }
        .per_day,
        None
    );
}
