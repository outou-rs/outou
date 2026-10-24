use outou::prelude::*;

#[component]
fn Choices() -> Element {
    <option value="x">X</option>
}

#[component]
fn App() -> Element {
    let cond = false;
    let extra = <option value="e">E</option>;
    <form>
        <select value="a">
            <option>No value</option>
            <option value="a">A</option>
            <Choices />
            {extra}
        </select>
        <select multiple value="a">
            <option value="a">A</option>
        </select>
        <select value="a"></select>
        <select multiple={false} value="a">
            <option value="a">A, a single choice: no warning</option>
        </select>
        <select multiple={cond} value="a">
            <option value="a">A</option>
        </select>
    </form>
}
