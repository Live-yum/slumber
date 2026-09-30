# 内置编辑、查看、查询与数据库工具（crypto.3）

本次修复 Windows 在 x 菜单编辑时尝试启动 vim 的问题。三个目标共用内置 Rust 编辑器、查看器、过滤和 SQL 工具，不需要安装 Vim、less、more、jq 或 sqlite3。验证是否通过以发布提交对应的 Actions 为准。

附带 `QUICKSTART.zh-CN.txt` 精简操作说明，UTF-8 BOM/CRLF，可直接在 Windows 文本编辑器中打开。crypto.3 继续使用 crypto.2 的内置工具实现，并修复内置编辑器与文件监视器重复重载导致菜单丢失的问题；三个目标都重复运行真实终端保存/取消和 CLI 编辑验收三次。

## 编辑

TUI 按 x 选择 Edit Recipe / Edit Profile / Edit Collection；请求 body 的 e、多行字段编辑、CLI `collection --edit` / `config --edit` 也走内置编辑器。

Ctrl+S 保存，F2 保存并关闭，Esc 或 Ctrl+Q 关闭。有未保存修改时：Y 放弃未保存修改，N 继续编辑，S 保存并关闭。已经 Ctrl+S 保存的修改不因随后取消而撤销。

方向键、Home/End、PageUp/PageDown 移动；Shift+方向键选择；Ctrl+A 全选；Ctrl+Z/Y 撤销/重做；Ctrl+F 查找，F3 下一处；Tab 插入两个空格。Ctrl+C/X/V 使用编辑器内部剪贴板。从系统粘贴使用终端粘贴快捷键（例如 Windows Terminal 的 Ctrl+Shift+V），支持 bracketed paste，不调用外部剪贴板工具。

按 UTF-8 编辑，支持中文、组合字符、CRLF/LF、BOM 和中文空格路径。不进行 shell 路径拼接。保存先写临时文件再原子替换，保留权限；检测到磁盘内容被其他程序改动时拒绝覆盖并保留编辑缓冲区。取消不写入文件。无修改不重写，不建立自动恢复或交换文件。显式编辑会向操作者显示配置内容，包括自行填写的密钥。

集合保存后重新加载；YAML 不合法时显示原有配置错误，可重新编辑。body 编辑是会话内覆盖，不擅自改写集合。

## 查看、查询和导出

View Body/v 打开只读 `Built-in viewer`；方向键、翻页、Ctrl+F/F3 搜索，Esc/Q 关闭。非法 UTF-8 二进制显示十六进制，不调用 pager。

响应 / 查询使用内置 Rust JaQ：`.data.list`、`.items | map(.name)`、`jq '.data.list'`、`jq -r '.token'`。支持 -r/-c，不启动 jq，也不宣称实现 jq CLI 的所有参数。原始/派生视图仍按所选内容查询。

冒号导出输入 `save C:\路径 含空格\response.json` 或 `save /path/response.json`，可给整个路径加双引号；Windows 反斜杠不作为 shell 转义。原子创建新文件，已存在时拒绝覆盖；需要覆盖时使用原有 Save Body 对话框并确认。`cat`、`head -c N`、`head -n N`、`tee PATH` 也有内置处理。

## 数据库

`slumber --portable db` 打开内置 SQL 控制台；`slumber --portable db 'SELECT 1;'` 执行后退出，也可从 stdin 读 SQL。支持多条语句以及 .tables、.schema、.help、.quit/.exit。交互 SQL 以分号结束。

结果为 columns/rows JSON，保留重复列名、64 位整数、NULL；BLOB 为 X'十六进制'。写入事务请显式 BEGIN/COMMIT。使用编译内置 SQLite，不加载扩展，不复制 sqlite3 CLI 的所有 dot-command。

## 默认值和边界

默认内置 editor/pager，不再通过 EDITOR/VISUAL/PAGER 自动查找外部程序。可以显式写 `editor: builtin`、`pager: builtin`。便携模式始终内置，即使旧配置是 `editor: vim`。

普通模式保留明确配置的外部 editor/pager、db --exec 和 `command([...])` 高级集成；响应外部 shell 需 `!命令`。便携模式拒绝 command()、db --exec 和 !shell，不会尝试运行缺失程序。这些“执行用户自定义程序”的能力不等于随包内置 Python/Java/Node 或通用 shell。正常请求、加解密、编辑、查看、查询、导出、历史和 SQL 有不依赖额外程序的实现。操作系统和终端本身仍是必要环境，系统剪贴板支持取决于终端。

## 升级与验证

解压到新目录，复制自己的 slumber.yml/config.yml 和所需 data；不要用示例覆盖真实配置。只替换 slumber.exe/slumber 也可保留原有私有配置。

Actions 在 Windows AMD64、Linux ARM64、Linux x86_64 保留原加解密回归，新增实际 TUI 控制器的 x 菜单编辑、保存/取消/重载、body 编辑/查看/发送、内置过滤、Unicode、磁盘冲突和 SQL 测试。协议 smoke 子进程清空 PATH，并让 EDITOR/VISUAL/PAGER 指向不存在的程序。只有对应提交测试、打包和依赖审计通过后才发布。
