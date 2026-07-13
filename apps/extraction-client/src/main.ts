import "./styles/index.css";

const root = document.querySelector<HTMLElement>("#app");
if (root === null) {
  throw new Error("缺少应用根节点 #app");
}

if (import.meta.env.MODE === "e2e") {
  void import("./testing/e2e-main").then(({ startE2eClient }) =>
    startE2eClient(root),
  );
} else {
  void import("./app/controller").then(({ ProductController }) => {
    const controller = new ProductController(root);
    return controller.start();
  });
}
