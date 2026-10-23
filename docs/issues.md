# GitHub Issues 镜像

> 仓库：danielbill/pi-flash ｜ 同步：2026-10-23 20:51 ｜ 共 4 条（open 2）
>
> 本文件由 `scripts\sync-issues.bat` 自动生成，手动修改会被下次同步覆盖；评论/关闭请走 `gh issue`。

## 🔴 Open（2）

### #4 显示的md渲染颜色可以自定义最好
- 状态：open ｜ 标签：— ｜ 作者：zhoujun681 ｜ 创建：2026-10-23 ｜ 更新：2026-10-23
- https://github.com/danielbill/pi-flash/issues/4

<img width="1975" height="549" alt="Image" src="https://github.com/user-attachments/assets/8d95053d-0f02-465c-a411-8dc6958d7155" />
暗主题这些字颜色太不好看了，可以自定义最好。代码块最好带代码框显示。图上源码为：
```bash
  cd /opt/projects/daily-report-test/daily-report-src
  bash deploy/build-image.sh --root /opt/projects/daily-report-test
  bash deploy/upgrade.sh --root /opt/projects/daily-report-test "$(bash deploy/version.sh read)"
  curl -fsS http://127.0.0.1:8091/api/system/health
```

### #2 add release build for mac?
- 状态：open ｜ 标签：— ｜ 作者：mungowang ｜ 创建：2026-10-20 ｜ 更新：2026-10-21
- https://github.com/danielbill/pi-flash/issues/2

<details><summary>评论（1）</summary>

**@danielbill**（2026-10-21）：

由于没有linux环境，暂无计划

</details>

## ✅ Closed（2）

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

