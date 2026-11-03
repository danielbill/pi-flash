# GitHub Issues 镜像

> 仓库：danielbill/pi-flash ｜ 同步：2026-11-03 16:43 ｜ 共 8 条（open 5）
>
> 本文件由 `scripts\sync-issues.bat` 自动生成，手动修改会被下次同步覆盖；评论/关闭请走 `gh issue`。

## 🔴 Open（5）

### #8 没设置模型费用，会导致Error Cannot read properties of undefined (reading 'tiers')
- 状态：open ｜ 标签：— ｜ 作者：zengdaming ｜ 创建：2026-10-26 ｜ 更新：2026-10-26
- https://github.com/danielbill/pi-flash/issues/8

### 背景：
我使用自定义openai接口，但是没有设置模型的费用（例如：使用CLI Proxy API ，自建API的聚合）
会无法成功运行，出现错误：
Error Cannot read properties of undefined (reading 'tiers')

### 当前解决方法是
providers.json（或 models.json）的模型定义中补齐 cost 字段：
```
"cost": {
  "input": 0,
  "output": 0,
  "cacheRead": 0,
  "cacheWrite": 0
}
```
### 建议
能兼容么有定义模型费用的情况，提高健壮性

### #7 装了扩展，使用的时候会话页面卡住了
- 状态：open ｜ 标签：— ｜ 作者：yuanjingking ｜ 创建：2026-10-26 ｜ 更新：2026-10-26
- https://github.com/danielbill/pi-flash/issues/7

<img width="2418" height="1395" alt="Image" src="https://github.com/user-attachments/assets/769fcb2f-9748-4d33-bbda-cfef23d4d788" />

### #6 建议加上当前会话消耗的token数
- 状态：open ｜ 标签：— ｜ 作者：yuanjingking ｜ 创建：2026-10-25 ｜ 更新：2026-10-25
- https://github.com/danielbill/pi-flash/issues/6

建议加上当前会话消耗的token数，每个会话的token消耗，使用的模型

### #5 建议支持自定义供应商
- 状态：closed ｜ 标签：— ｜ 作者：easyeda2021 ｜ 创建：2026-10-23 ｜ 更新：2026-10-23 ｜ 关闭：2026-11-03
- https://github.com/danielbill/pi-flash/issues/5

<details><summary>评论（1）</summary>

**@danielbill**（2026-11-03）：

自定义供应商已支持
</details>

抖音刷到找来的
1、不支持自定义provider，导致企业统一内部中转站的路由用不了
2、界面看起来有些粗糙，建议可以让AI再美化一下
3、可以配套vscdoe插件使用

### #2 add release build for mac?
- 状态：open ｜ 标签：— ｜ 作者：mungowang ｜ 创建：2026-10-20 ｜ 更新：2026-10-21
- https://github.com/danielbill/pi-flash/issues/2

<details><summary>评论（1）</summary>

**@danielbill**（2026-10-21）：

由于没有linux环境，暂无计划

</details>

## ✅ Closed（3）

### #4 显示的md渲染颜色可以自定义最好
- 状态：closed ｜ 标签：— ｜ 作者：zhoujun681 ｜ 创建：2026-10-23 ｜ 更新：2026-10-24 ｜ 关闭：2026-10-24
- https://github.com/danielbill/pi-flash/issues/4

<img width="1975" height="549" alt="Image" src="https://github.com/user-attachments/assets/8d95053d-0f02-465c-a411-8dc6958d7155" />
暗主题这些字颜色太不好看了，可以自定义最好。代码块最好带代码框显示。图上源码为：
```bash
  cd /opt/projects/daily-report-test/daily-report-src
  bash deploy/build-image.sh --root /opt/projects/daily-report-test
  bash deploy/upgrade.sh --root /opt/projects/daily-report-test "$(bash deploy/version.sh read)"
  curl -fsS http://127.0.0.1:8091/api/system/health
```

<details><summary>评论（2）</summary>

**@danielbill**（2026-10-23）：

eidtor markdown：

<!-- Failed to upload "ScreenShot_2026-10-23_211741_932.png" -->

session markdown：

<!-- Failed to upload "ScreenShot_2026-10-23_211852_561.png" -->

这很奇怪，我的editor md渲染是白色， session内容渲染是黄色的， 很可惜截图上传失败。
我会先让agent定位颜色差异问题。

**@danielbill**（2026-10-24）：

渲染色已改为亮紫黑金

</details>

### #3 会话自动命名
- 状态：closed ｜ 标签：— ｜ 作者：pg1737151964 ｜ 创建：2026-10-22 ｜ 更新：2026-10-22 ｜ 关闭：2026-10-22
- https://github.com/danielbill/pi-flash/issues/3

pi-web会话调用模型自动命名挺好用的，希望能集成进来

<details><summary>评论（1）</summary>

**@danielbill**（2026-10-22）：

PF无计划实现此功能，我正是那个提出反对的意见的用户 ：）

</details>

### #1 打开终端后，输入exit会程序崩溃退出
- 状态：closed ｜ 标签：— ｜ 作者：hailwind ｜ 创建：2026-10-20 ｜ 更新：2026-10-20 ｜ 关闭：2026-10-20
- https://github.com/danielbill/pi-flash/issues/1

如题，用exit会整个程序退出

<details><summary>评论（1）</summary>

**@danielbill**（2026-10-20）：

0.2.1已修

</details>

