import type { Calm } from "@calm/plugin-sdk";
import { render } from "preact";
import { useEffect, useState } from "preact/hooks";

function Hello({ calm }: { calm: Calm }) {
  const [clicks, setClicks] = useState(0);

  useEffect(() => {
    calm.panel.setTitle("My Plugin");
  }, [calm]);

  function click() {
    setClicks(clicks + 1);
    calm.log(`clicked ${clicks + 1} times`);
  }

  return (
    <div>
      <p>Hello from a TSX plugin.</p>
      <button type="button" onClick={click}>Clicked {clicks} times</button>
    </div>
  );
}

const root = document.getElementById("app");
if (root) {
  if (window.calm) {
    render(<Hello calm={window.calm} />, root);
  } else {
    render(<p>Open this plugin inside CALM.</p>, root);
  }
}
