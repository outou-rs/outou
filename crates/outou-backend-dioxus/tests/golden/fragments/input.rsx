use outou::prelude::*;

/// A top-level fragment is several roots.
#[component]
fn Rows() -> Element {
    <>
        <tr><td>a</td></tr>
        <tr><td>b</td></tr>
    </>
}

/// A single-child fragment, an empty one, and text and an expression as roots.
#[component]
fn Shapes(n: u8) -> Element {
    let one = <><p>only</p></>;
    let none = <></>;
    <>
        {one}
        {none}
        tail {n}
    </>
}

/// A fragment child is spliced into its parent; an empty one adds nothing.
#[component]
fn Spliced() -> Element {
    <ul>
        <><li>a</li><li>b</li></>
        <li>c</li>
        <><></></>
        <><><li>d</li></></>
    </ul>
}

#[component]
fn Wrap(content: Element) -> Element {
    <div>{content}</div>
}

/// A fragment as the body of a map closure and in an attribute island.
#[component]
fn Lists(items: Vec<u8>) -> Element {
    <dl>
        <Wrap content={<><b>x</b><i>y</i></>} />
        {items.iter().map(|n| <><dt key={*n}>{*n}</dt><dd>!</dd></>)}
    </dl>
}

/// A key after the first root is kept out of the generated call.
#[component]
fn LaterKey() -> Element {
    <><i/><i key="ignored"/></>
}

/// Options inside fragments belong to the enclosing `<select value>`.
#[component]
fn Picker(levels: Vec<u8>, current: u8) -> Element {
    <select value={current}>
        <>
            <option value="0">none</option>
        </>
        {levels.iter().map(|level| <><option value={*level}>{*level}</option></>)}
    </select>
}
