// Behavior test for plugins/calm-keys/index.html, run under bun by first_party_pages.rs.
// It runs the page's inline script against a small fake DOM and a fake `window.calm`.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const pagePath = process.argv[2];
const html = readFileSync(pagePath, "utf8");

class Node {
  constructor(tag, document) {
    this.tagName = tag;
    this.ownerDocument = document;
    this.children = [];
    this.className = "";
    this.id = "";
    this.text = "";
  }
  get textContent() {
    return this.text + this.children.map((child) => child.textContent).join("");
  }
  set textContent(value) {
    this.children = [];
    this.text = String(value);
  }
  // The page must build nodes, never parse markup from host data.
  set innerHTML(_value) {
    throw new Error("the page set innerHTML");
  }
  appendChild(child) {
    if (child.tagName === "#fragment") {
      this.children.push(...child.children);
      child.children = [];
    } else {
      this.children.push(child);
    }
    this.ownerDocument.register(child);
    return child;
  }
  replaceChildren(...nodes) {
    this.children = [];
    this.text = "";
    nodes.forEach((node) => this.appendChild(node));
  }
  findAll(predicate, out = []) {
    this.children.forEach((child) => {
      if (predicate(child)) out.push(child);
      child.findAll(predicate, out);
    });
    return out;
  }
}

// Static markup the script reads by id: a node per element id in the page.
function makeDocument() {
  const document = {
    byId: {},
    register(node) {
      if (node.id) this.byId[node.id] = node;
      node.children.forEach((child) => this.register(child));
    },
    createElement(tag) {
      return new Node(tag, document);
    },
    createDocumentFragment() {
      return new Node("#fragment", document);
    },
    getElementById(id) {
      return this.byId[id] || null;
    },
    documentElement: { style: { setProperty() {} } },
  };
  for (const match of html.matchAll(/<(\w+)[^>]*\bid="([^"]+)"/g)) {
    const node = new Node(match[1], document);
    node.id = match[2];
    document.byId[node.id] = node;
  }
  return document;
}

const script = /<script>([\s\S]*?)<\/script>/.exec(html)[1];
// Drawing hints written into the static markup, tags stripped. The page should have none.
const staticDrawHints = [...html.matchAll(/<li>([\s\S]*?)<\/li>/g)]
  .map((match) => match[1].replace(/<[^>]+>/g, ""))
  .filter((text) => /draw/i.test(text));

const defaults = [
  { id: "canvas.draw_terminal", label: "Draw a terminal", group: "Canvas", key: "T" },
  { id: "canvas.draw_browser", label: "Draw a browser", group: "Canvas", key: "B" },
  { id: "view.fit_all", label: "Fit all panels", group: "View", key: "⇧⌘0" },
];

// Runs the page once and returns its visible text after the promises settle.
async function runPage(calm) {
  const document = makeDocument();
  const window = { calm, addEventListener() {} };
  new Function("window", "document", "calm", script)(window, document, calm);
  await new Promise((resolve) => setTimeout(resolve, 0));
  const textOf = (id) => {
    const node = document.getElementById(id);
    assert.ok(node, `page has no #${id}`);
    return node
      .findAll((child) => child.tagName === "li" || child.tagName === "p")
      .map((child) => child.textContent);
  };
  const drawNode = document.getElementById("draw");
  const dynamicDraw = drawNode ? textOf("draw") : [];
  return {
    keys: textOf("keys").concat([document.getElementById("keys").textContent]),
    draw: staticDrawHints.concat(dynamicDraw),
  };
}

const answer = (keys) => ({ keys: { list: () => Promise.resolve({ keys }) } });
const cases = {};

cases["default keys"] = async () => {
  const page = await runPage(answer(defaults));
  assert.deepEqual(page.draw, ["T, then drag draws a terminal", "B, then drag draws a browser"]);
  assert.ok(page.keys.includes("Fit all panels⇧⌘0"), page.keys.join(" | "));
  // Move gestures follow a setting, so the page names it instead of promising Cmd.
  assert.match(html, /Cmd\+drag \(default\)/);
  assert.match(html, /canvas\.move_modifier/);
};

cases["rebound draw key"] = async () => {
  const keys = defaults.map((row) => (row.id === "canvas.draw_terminal" ? { ...row, key: "X" } : row));
  const page = await runPage(answer(keys));
  assert.deepEqual(page.draw, ["X, then drag draws a terminal", "B, then drag draws a browser"]);
};

cases["unbound draw key"] = async () => {
  const keys = defaults.map((row) => (row.id === "canvas.draw_browser" ? { ...row, key: null } : row));
  const page = await runPage(answer(keys));
  assert.deepEqual(page.draw, ["T, then drag draws a terminal", "Draw a browser: not bound"]);
};

cases["host text stays literal"] = async () => {
  const keys = [
    { id: "canvas.draw_terminal", label: "Draw", group: "<b>G</b>", key: "<img src=x>" },
    { id: "custom", label: "<script>x</script>", group: "<b>G</b>", key: "&amp;" },
  ];
  const page = await runPage(answer(keys));
  assert.deepEqual(page.draw, ["<img src=x>, then drag draws a terminal", "Draw a browser: not bound"]);
  assert.ok(page.keys.includes("<script>x</script>&amp;"), page.keys.join(" | "));
};

cases["missing calm.keys"] = async () => {
  const page = await runPage({});
  assert.deepEqual(page.draw, []);
  assert.ok(page.keys.some((text) => text.includes("no calm.keys.list()")), page.keys.join(" | "));
};

cases["rejected call"] = async () => {
  const page = await runPage({ keys: { list: () => Promise.reject(new Error("boom")) } });
  assert.deepEqual(page.draw, []);
  assert.ok(page.keys.includes("Could not load key bindings: boom"), page.keys.join(" | "));
};

let failed = 0;
for (const [name, run] of Object.entries(cases)) {
  try {
    await run();
    console.log(`ok - ${name}`);
  } catch (error) {
    failed += 1;
    console.log(`not ok - ${name}: ${error.message}`);
  }
}
console.log(`${Object.keys(cases).length - failed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
