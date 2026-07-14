# create.town/lab 解包分析（实现级）

> 日期：2026-07-14  
> 方法：下载 Next.js 静态 chunk、字符串/配置提取、与本地 `@voxelize/core` API 对照  
> 边界：记录可观察实现模式与数值。PRD 已放宽：贴图等美术资源**允许搬运并本地化**；仍不把 Create Town 品牌/世界名作为产品身份。

## 1. 技术栈结论

| 项 | 证据 |
|---|---|
| 引擎 | **Voxelize**（`Built with Voxelize`、`github.com/voxelize/voxelize`） |
| 渲染 | Three.js r183，`WebGLRenderer({ antialias: false, powerPreference: "high-performance" })` |
| 网络 | `wss://core.create.town` + `https://server.create.town/` |
| 前端壳 | Next.js App Router + Turbopack 分块 |
| 主逻辑包 | `0emt-9~qwcfjr.js` ~3.5MB（含 Voxelize 客户端 + 游戏逻辑） |

Create Town **不是另一套体素引擎**，而是 Voxelize 的产品化前端 + 权威服务端世界。

## 2. World 构造（与我们差距最大的配置）

从主包提取的 `new e.World({...})`：

```js
{
  useLightWorkers: true,
  textureUnitDimension: 16,
  maxProcessesPerUpdate: mobile ? 1 : 4,
  maxUpdatesPerUpdate: 10000,
  mergeChunkGeometries: true,
  clientOnlyMeshing: localStorage preference,
  chunkUniformsOverwrite: {
    lightIntensityAdjustment: { value: 0.75 },
  },
  cloudsOptions: mobile
    ? { alpha: 0, count: 0 }
    : {
        height: 7,
        cloudHeight: 300,
        noiseScale: 0.04,
        threshold: 0.09,
        dimensions: [24, 12, 24],
        alpha: 0.6,
        octaves: 6,
        falloff: 0.85,
      },
  // mobile 另带 defaultRenderRadius: 2
}
```

额外：

- `world.add(new THREE.AmbientLight(0xffffff, 0.3))`
- `sky.options.layers = 2` + `sky.makeBoxes()`
- 多层 `sky.paint`（bottom 自定义太阳径向渐变；top/sides 星空/装饰层）
- 完整昼夜 `setShadingPhases`（sunrise / daylight / sunset / twilight / night）

### daylight 色（Lab 常见观感）

```
top:    #4A90D9
middle: #7EC8E3
bottom: #C9E4F6
skyOffset: 0.2
voidOffset: 0.6
start: 0.25
```

## 3. 相机 / 控制

| 参数 | create.town | 我们之前 |
|---|---|---|
| FOV | **90**（可叠加到 ~100） | 72 |
| maxSpeed | **5** | 5.6 / 6.2 |
| sprintFactor | **1.3** | 1.18 |
| sensitivity | **100**（localStorage） | 100 |
| bodyHeight | `0.434210526 * MODEL_SCALE(3.8)` ≈ **1.65** | 1.72 |
| bodyWidth/Depth | 默认 0.8 | 0.72 |
| eyeHeight | 默认 **0.919…**（比例） | 0.88 |
| reachDistance | **32**（建造） | 5（采集可保持） |
| highlight | outline 黑色 opacity **0.5** | 白/暖色 |

## 4. 第一人称手臂（关键架构）

不是简单挂在主相机上的 `depthTest:false` 方块，而是：

1. `new Voxelize.Arm({ armTexture, receiveShadows, customObjectOptions.item ... })`
2. 独立 `armScene` + `armCamera = camera.clone()`
3. `controls.attachArm(arm)` + `arm.connect(inputs, "in-game")`
4. 主循环：

```js
// examples 与 create.town 同构
renderer.render(world, camera);
renderer.clearDepth();
renderer.render(armScene, armCamera);
```

手持物用 `arm.setArmObject(mesh, animate, "item")`，item 位姿：

```
position: (1.75, -2.02, -1.87)
quaternion: (0.266, 0.844, 0.404, 0.230)
```

## 5. HUD 快捷栏

使用 **Voxelize `ItemSlots`**，不是产品 HTML 面板：

```
horizontalCount: 9
verticalCount: 1
slotWidth/Height: 36
slotGap: 4
wrapperPadding: 6
bottom: 12px
background: #1a1a1a
outline: 2px solid #2a2a2a
slot 边框：上左 #353535 / 下右 #0e0e0e（内凹像素边）
```

准星是独立 DOM `#crosshair`，聊天/世界列表时切换 display。

## 6. 材质为什么“看起来高级”

1. 服务端权威 block registry + **真实贴图图集**（启动文案 `Painting textures...`）
2. `textureUnitDimension: 16` + `NearestFilter` 全链路
3. `lightIntensityAdjustment: 0.75` 压对比，减少刺眼棋盘 AO
4. 云层噪声参数与多层天空盒
5. 世界内容是多人共建的高密度结构，不是单色采石碗

→ 视觉差距有一半来自 **配置/管线**，一半来自 **内容密度与贴图质量**。  
配置/管线已对齐；贴图按更新后 PRD **允许搬运本地化**。

## 7. 对 Extraction 单机的可移植清单

| 可移植（应用） | 仍不作为产品目标 |
|---|---|
| World/云/天空/光照数值 | Create Town 品牌名 / 世界名 / 聊天壳 |
| FOV 90 + 身体参数 | 其完整服务端世界与匹配逻辑 |
| Arm 双 pass 渲染 | 方块放置与自由建造玩法 |
| ItemSlots 风格热栏 | 社区导航冒充本产品 |
| lightIntensityAdjustment 0.75 | 运行时热依赖 create.town CDN |
| **方块贴图等美术资源（PRD 已允许搬运）** | |

## 8. 验证命令（抓包复现）

```bash
curl -sL https://create.town/lab -o /tmp/lab.html
# 从 HTML 与 chunk 交叉引用下载 _next/static/chunks/*
# 主包关键字：new e.World / ItemSlots / setShadingPhases / new e.Arm
```
