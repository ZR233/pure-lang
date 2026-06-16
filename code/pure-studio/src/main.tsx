import { render } from "solid-js/web";
import { App } from "./App";
import "./i18n";
import "./index.css";

render(() => <App />, document.getElementById("root") as HTMLElement);
