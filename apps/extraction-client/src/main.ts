import "./styles/index.css";

const root = document.querySelector<HTMLElement>("#app");
if (root === null) {
  throw new Error("缺少应用根节点 #app");
}

if (import.meta.env.MODE === "e2e") {
  void import("./testing/e2e-main").then(({ startE2eClient }) =>
    startE2eClient(root),
  );
} else if (import.meta.env.MODE === "live-e2e") {
  void import("./testing/live-e2e-main").then(({ startLiveE2eClient }) =>
    startLiveE2eClient(root),
  );
} else {
  const parameters = new URLSearchParams(window.location.search);
  if (import.meta.env.DEV && parameters.get("mode") === "single") {
    void import("./single/main").then(({ startSinglePlayerClient }) =>
      startSinglePlayerClient(root),
    );
  } else if (import.meta.env.DEV && parameters.get("mode") === "dev-mp") {
    // 跳过钱包 UI：确定性 SIWE + 自动入队，专注局内多人表现
    void import("./testing/dev-multi-main").then(({ startDevMultiplayerClient }) =>
      startDevMultiplayerClient(root),
    );
  } else {
    void import("./app/controller").then(({ ProductController }) => {
      const controller = new ProductController(root);
      return controller.start();
    });
  }
}
