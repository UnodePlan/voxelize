# 本地 World 运行时调研

## Conclusion

第一阶段无需修改 Voxelize Core 的公开初始化契约，也无需启动本地 Rust Server。客户端可以在专用单机适配层中构造与现有协议一致的本地 `INIT` 和 `LOAD` 数据，把确定性区块送入真实 `World`，然后继续复用 `RigidControls`、`VoxelInteract`、物理碰撞、光照、meshing worker、Sky、Clouds 和渲染循环。

该方案把“本地权威”限制在 `?mode=single` 的运行时内，生产 `VoxelWorldSession` 仍只接受真实网络消息。

## Repository Evidence

- `World.initialize()` 要求先收到初始化数据，否则抛出 `World has not received any initialization data from the server.`（`packages/core/src/core/world/index.ts:3610-3630`）。
- `World.onMessage({ type: "INIT", json })` 只是把 `json` 保存为 `initialData`；因此本地适配器可以通过同一公开消息入口提供 blocks、items、options 和 stats，而不需要绕过私有字段（`packages/core/src/core/world/index.ts:3800-3820`）。
- `World.onMessage({ type: "LOAD", chunks })` 会把区块交给现有 ChunkPipeline；`World.update()` 随后完成客户端 meshing、物理、Sky、Clouds 和 uniforms 更新（`packages/core/src/core/world/index.ts:3830-3850`，`4060-4230`）。
- `RawChunk`/`Chunk` 已提供确定性 voxel/light 数组结构，ChunkProtocol 可以由本地地图生成器构造（`packages/core/src/core/world/raw-chunk.ts`，`packages/core/src/core/world/chunk.ts`）。
- `World.updateVoxel(s)` 会进入现有本地更新、光照和 remesh 管线；单机适配层需要阻断或消费由 World 产生的网络 packets，避免把 LOAD/UPDATE 解释为真实网络流量（`packages/core/src/core/world/index.ts:2780-2860`）。
- 更新使用 `source: "server"` 时仍执行本地 voxel、光照和 remesh，但 `processClientUpdates` 只把 `source: "client"` 的更新加入待发送队列；单机挖掘可以使用 server source 获得真实本地修改且不产生 UPDATE 网络包（`packages/core/src/core/world/index.ts:5589-5623`）。
- 示例客户端已证明 Core 支持 `textureUnitDimension: 8`、Sky shading phases、`RigidControls`、`VoxelInteract`、ItemSlots、像素纹理和第一人称交互（`examples/client/src/main.ts`）。
- 生产 Extraction World 会话已经封装了相机、控制器、VoxelInteract、actor、resize、render 和 dispose，但它依赖服务端 `INIT`、Peer 与 network actions；单机应抽取可共享的纯视觉/控制参数，不能让生产会话接受本地结果（`apps/extraction-client/src/game/world-session.ts`）。

## Recommended Boundary

- `single/runtime.ts`：拥有单机生命周期、渲染循环、时钟、结果转换和 dispose。
- `single/world.ts`：构造 World、本地 INIT、确定性 chunks、Sky/Clouds/材质和 extraction beacon。
- `single/map.ts`：纯函数生成废弃采石场体素布局、出生点、撤离区和资源分布。
- `single/input.ts`：复用 RigidControls 与 VoxelInteract，但把挖掘/丢弃发送到本地规则，而不是生产网络。
- `single/state.ts`：管理 12 格背包、当前目标、挖掘进度、经过时间和三秒撤离状态。
- `single/view.ts`：渲染 Create Town 参考风格的最小 HUD、快捷栏、提示和结果。

以上文件名是设计候选，最终边界在 `design.md` 中确定。

## Risks

- 构造本地 INIT 时 block 定义必须与 mesher/physics 需要的字段完整兼容，不能只提供资源清单中的 ID。
- World 会主动请求 LOAD 并排出 packets；单机适配器必须确定性响应或预加载所需 chunks，不能让请求无限累积。
- 更新方块会触发光照和邻区 remesh；地图边界与 chunk 边界必须覆盖挖掘回归。
- Core 生命周期包含 worker、atlas、CSM 和 chunk pool，重复重开必须验证完全释放。

