use outou::prelude::*;

#[component]
fn App() -> Element {
    <div>
      <FirstComponentWithLongName/>

      {" "}
      <SecondComponentWithLongName/>

      <ThirdComponentWithLongName/>
    </div>
}
