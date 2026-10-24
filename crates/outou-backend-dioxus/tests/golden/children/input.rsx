use outou::prelude::*;

unsafe fn raw_three() -> usize {
    3
}

#[component]
fn Row(label: String) -> Element {
    <li>{label}</li>
}

#[component]
fn App() -> Element {
    let count: usize = 3;
    let labels = vec![String::from("a"), String::from("b")];
    let open = count > 1;
    let debug = false;
    let ready = false;
    let cond = false;
    let value = 1;
    let maybe_n: Option<usize> = Some(2);
    <div>
        {count}
        {3.5}
        {"literal"}
        {count.to_string()}
        {labels.iter().map(|label| <Row key={label.as_str()} label={label.clone()} />)}
        {if open { <b>{count}</b> } else { <i>closed</i> }}
        {let shown = open.then(|| <u>shown</u>); shown}
        {/* a comment child */}
        {}
        {if debug { println!("render") } "x"}
        {if ready {} "done"}
        {while cond {} "done"}
        {loop { break } "done"}
        {match value { _ => {} } "done"}
        {fn h() -> &'static str { "x" } h()}
        {"a;{}"}
        {';'}
        {r#"x;}"#}
        {'a: { if open { break 'a 1 } 2 }}
        {unsafe { raw_three() }}
        {loop { break 4 }}
        {if let Some(n) = maybe_n { n } else { 0 }}
        {(|x: usize| { x + 1 })(count)}
        {vec![1; 3].len()}
        {format!("{};", count)}
        {{ let inner = count; inner }}
        <Row label={String::from("nested")} />
    </div>
}
