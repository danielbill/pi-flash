# project manager
管理项目打开，查找、列表、隐藏、删除等。

模块代码：projectManager

触发按钮：
1、项目会话列表上方的 【打开项目】icon
2、新会话页 inputpanel 下方的 【打开项目】 icon

## 打开项目
见 【打开项目菜单.png]
内容：
- 搜索框
- 打开文件夹按钮 【打开文件夹】
- 本机项目列表：按字母排序列出最近30天打开过的项目，限高10条，超长显示滚动条

高500px， 宽500px，居中显示


注：如果是从某项目新建会话，则默认选中该项目


## 隐藏项目

该动作触发来自 项目会话列表 ，项目控制栏 增加 隐藏按钮
<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="lucide lucide-eye-off preview-icon"><path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/></svg>

点击后从列表中隐藏该项目不再展示。
增加【项目管理配置】，标记该项目 hidden
系统启动时默认加载N天内活动的会话，要过滤 hidden project







