use outou::prelude::*;

#[component]
fn Selects(current: String, levels: Vec<u8>) -> Element {
    <form>
        <select value={current.clone()}>
            {["Low", "High"].into_iter().map(|level| <option key={level} value={level}>{level}</option>)}
        </select>
        <select value={3u8}>
            <option value="1">One</option>
            <option value={2u8}>Two</option>
            <option value="3" selected={false}>Three, explicit</option>
            <option>No value</option>
            <optgroup label="More">
                {levels.iter().map(move |level| <option value={*level}>{*level}</option>)}
            </optgroup>
        </select>
        <select value="static">
            <option value="static">Static</option>
        </select>
        <select value={ let base = 1u8; base + 1 }>
            <option value="2">Two</option>
        </select>
        <select multiple value={current}>
            <option value="Low">Low</option>
        </select>
        <select value={*levels.first().unwrap()}>
            {levels.iter().map(|level| <option value={*level}>{*level}</option>)}
        </select>
        <div>
            {levels.iter().map(|level| <select value={*level}>
                {levels.iter().map(|other| <option value={*other}>{*other}</option>)}
            </select>)}
        </div>
    </form>
}
