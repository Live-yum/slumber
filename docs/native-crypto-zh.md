# Slumber 原生加解密便携版

**crypto.2 更新：** x 菜单编辑、body 编辑/查看、响应查询与 SQL 已提供内置工具，不再默认调用 vim、less/more、jq 或 sqlite3。编辑器 F2 保存并关闭，Ctrl+S 保存，Esc 关闭；有未保存修改时确认放弃。完整快捷键、导出语法和升级说明见随包 `BUILTIN-TOOLS.zh-CN.md`（源码 `docs/builtin-tools-zh.md`）。不要用示例文件覆盖自己的真实配置。

此 fork 保留 Slumber 的 TUI、CLI、导入、模板、profiles、请求串联及查询能力。加解密在二进制内完成；成品运行不需要安装 Rust、Python、Node、Java、OpenSSL、jq 或 sqlite3。构建和独立测试可以使用这些开发工具。Base64/Base64URL 是编码，不提供保密性。

## 配置 → 选择请求 → Enter → 查看解密结果

解压适合系统/架构的压缩包。编辑可执行文件旁边的 `slumber.yml`，然后运行：

```powershell
.\slumber.exe --portable
```

```sh
./slumber --portable
```

选择 `am_persons` 或 `demo_b_realdata`，按 Enter。登录串联、参数加密、整包解码均按集合自动执行。配置了 `response_transform` 的 Response Body 默认显示 `Transformed JSON`；聚焦响应窗格（`2`）后按 F6 切换原始/派生视图。无需输入 `/` 或手工解密。后续查询、复制、保存使用当前视图；转换失败显示错误，F6 仍可查看原始网络报文。

发布包中的示例全部指向 `127.0.0.1:18080`，因此开箱不会调用真实现场。测试用服务在源码的 `scripts/native_crypto_smoke.py --serve`；这是开发演示工具，不是使用真实接口时的运行依赖。

## 修改地址和凭据

在 `profiles.local.data` 修改 `am_base_url`、`gateway_url`、`demo_b_base_url`、`client_id`、`client_secret`、`username`、`password`、`destination`。密钥直接改 `crypto.am.key.value/iv.value` 和 `crypto.demo_b.key.value/iv.value`。所有数字开头/全数字的材料均用引号表示字符串。真实个人配置不要提交公开仓库。

AM 使用普通表单 POST 登录，`client_id/client_secret` 是请求头，顶层 `access_token` 用于 Bearer 人员查询。登录用户名、密码没有擅自加 AES。人员的 phone 与 idcardNo 分别加/解密。

演示服务 B 使用 GET `/api/userlogin?user=...&pass=...`；登录整包先解码，再从 JSON 顶层取 token，后续 token 放在 query，不是 Bearer。示例的内建 Rust `jq()` 校验 code=0 和非空字符串 token；不是调用外部 jq。目录接口可仅含 items，通用 crypto 引擎不硬编码业务 code。`config.yml` 用正式的根级 `follow_redirects: false` 关闭重定向，没有增加写入重试。

`am_push_person`、`am_push_array` 会创建/更新业务记录，`demo_b_logout` 改变登录状态。真实使用前必须人工确认地址及权限。本次自动测试仅访问本机模拟服务。实际服务 B 的 POST `/api/realdata`、`/api/hisdata`、`/api/SQL` 应仍使用服务端约定的普通 JSON，不因为配置了响应 codec 就加密请求；实际 JSON 字段请按部署协议填写，勿把模拟回显格式套用到现场。

## 密钥和算法

```yaml
crypto:
  am:
    algorithm: aes-128-cbc
    padding: pkcs7
    key: {value: '0123456789abcdef', encoding: utf8}
    iv: {value: '0123456789abcdef', encoding: utf8}
    plaintext_encoding: utf8
    ciphertext_encoding: base64
```

上面仅为公开测试值。算法支持 `none`、`base64`、`base64url`、`aes-128-cbc`、`aes-192-cbc`、`aes-256-cbc` 及对应 `-ecb`。AES key 分别为 16/24/32 字节；CBC IV 始终为 16 字节，ECB 必须移除 iv。示例 AES-192 key 可用 `'0123456789abcdef01234567'`，AES-256 可用 `'0123456789abcdef0123456789abcdef'`。

`encoding: utf8`（或 `text`）直接取 UTF-8，不做哈希、派生、补零、截断或自动 Hex 猜测。`encoding: hex` 的 `'30313233343536373839616263646566'`、`encoding: base64` 的 `'MDEyMzQ1Njc4OWFiY2RlZg=='` 与上述公开 key 等价，IV 同理。可选模板方式：

```yaml
key:
  value: "{{ env('OPTIONAL_KEY') | sensitive() }}"
  encoding: utf8
```

未用的 AES 定义不会读取其 env。材料渲染只允许 profile、env、sensitive 和纯字符串函数，禁止请求、外部命令、文件、日志及递归 codec 调用。每次按选定 profile 解析，不把密钥放进全局缓存。

演示服务 B 切换为 Base64 时，整个 `crypto.demo_b` 改为：

```yaml
algorithm: base64
base64_decode:
  ignore_ascii_whitespace: true
  allow_missing_padding: true
```

Base64URL 改 algorithm 为 base64url；明文仅 `algorithm: none`。AES 的 `ciphertext_encoding` 可选 base64/base64url。输出带标准 `=` 填充、无换行；明确启用的解码选项兼容 ASCII 空白和缺失填充，但非法字符/长度仍报错。URL 模式解码也接受标准 `+/`，输出仍为 `-_`。不 URLDecode 密文，不将 `+` 变成空格，不 trim 明文。

固定 IV、KEY=IV、ECB 都只是既有协议兼容，不是新协议推荐。CBC/ECB 没有认证标签，不提供完整性保护，不能保证识别所有错误密钥。封装不含随机 IV、nonce、salt、tag 或 OpenSSL Salted__ 头。

## 字段与整包

函数为 `encrypt(crypto_id, value)`、`decrypt(crypto_id, value)`、`encode(crypto_id, value)`、`decode(crypto_id, value)`。管道把输入放在最后，可直接写 `phone | encrypt('am')`。encrypt/decrypt 仅用于 AES UTF-8 文本；encode/decode 覆盖全部 codec 并允许 bytes。number/object/array 必须先明确组织所需字节，不会被加密函数悄悄 stringify。

body/query/header 均可调用，JSON 转义与 URL 编码复用 Slumber 原路径。整包请求用 Slumber 5.3 既有的字符串 raw-body 语法，**不是附件草案中的 `type: raw`**：

```yaml
body: "{{ file('demo-request.plain.json') | encode('demo_b') }}"
```

仅 `encrypted_echo_demo` 模拟接口使用整包加密请求。协议测试只验证响应 codec，不能据此声称真实服务接受这种加密请求。

响应规则默认 fields，需要 paths；`scope: body` 必须是第一步，显式 `parse: json`，可用 `text_encoding: utf8-sig` 只移除开头 BOM。先解码原始 bytes，再解析 JSON，原始 Content-Type=text/plain 也可显示派生 JSON。之后可显式继续字段转换。

可写 JSONPath 子集：`$`、`.field`、`[0]`、`[*]`、`["特殊字段"]`；不支持递归下降和筛选表达式。选择器重复命中只处理一次；不同操作/不同跳过策略冲突报错。`skip_missing` 只跳过未找到，`skip_null` 只跳过 null，`skip_blank` 只跳过空白字符串；类型、编码、填充、UTF-8 错误仍失败。一个字段失败则整次不产生成功结果。

`data.list` 是 JSON 数组文本时使用 `am_persons_string_list`：先 `type: parse_json` 选 `$.data.list`，再解密数组字段。不自动解析所有字符串。未选字段及大整数保持数值，可重新排版，但派生 bytes 不等于网络 bytes。

## CLI 和持久化

```sh
./slumber --portable request am_persons
./slumber --portable request am_persons --transformed
./slumber --portable request am_persons --transformed --output persons.json
./slumber --portable -f /path/to/private.yml request demo_b_realdata --transformed
```

CLI 默认输出原始 body；`--transformed` 才转换。转换失败退出码为 **3**，不输出部分 JSON，不替换已有输出文件；通常错误为 1，启用 `--exit-status` 的 HTTP 错误为 2。文件输出先完整转换和写入临时文件，再替换目标。

`response()` 默认取原始 bytes。单字段串联可 `response('am_persons') | jsonpath('$.data.list[0].phone') | decrypt('am')`；整包登录需 `response('demo_b_login', view='transformed')`。两者不会暗中读取已转换缓存造成二次解密。

原始状态码、headers、body 和 Content-Length 保留。派生视图不自动进入历史；示例全设 persist:false。CLI 只有明确 `--persist` 才保存原始交换记录。显式复制、保存或把解密值用于下一请求是用户主动导出，需自行保护目标。历史响应按当前配置重新派生，旧密钥丢失后不能承诺仍可解密。

## 便携目录与 TLS

`--portable` 在可执行文件目录读取 config.yml/default slumber.yml，数据库/UI 状态位于 data/state.sqlite，临时文件在 tmp、日志在 log；state 目录预留。支持任意工作目录、中文及空格路径。显式 -f 优先，相对 file() 仍相对于集合。普通模式沿用上游路径策略；便携模式不靠不稳定的 SLUMBER_DATA_DIRECTORY。

SQLite 编译进二进制。TLS 校验开启：便携模式用编译进二进制的 Mozilla 根证书，更新证书库需要更新依赖并重建；普通模式仍用平台信任。私有 CA 不属于内置公共信任库，应使用正常模式的系统信任配置，而非关闭校验。编辑器、pager、SQL 控制台和响应 JaQ 查询默认内置；普通模式保留显式外部集成。便携模式在调用前拒绝任意 command()、db --exec 和 !shell，不包含 Python/Java/Node 等外部脚本运行时。

## 构建与验证记录

工具链 Rust 1.90.0；格式化 nightly-2026-02-20。根应用默认包含 import、CLI、TUI。

```sh
cargo +nightly-2026-02-20 fmt --all -- --check
cargo clippy --workspace --exclude slumber_python --all-targets --all-features --locked
cargo test --workspace --exclude slumber_python --locked --no-fail-fast
cargo build --release --locked -p slumber --target aarch64-unknown-linux-musl
python -m pip install cryptography PyYAML
python scripts/native_crypto_smoke.py --binary target/aarch64-unknown-linux-musl/release/slumber
```

Linux 需要构建期 musl-gcc，Windows 构建以 `-C target-feature=+crt-static` 固定 CRT。Actions 在对应原生 hosted runner 执行协议模拟和 TUI 事件循环测试，不把交叉编译冒称 ARM 麒麟真机验收。

发布需全部必需任务通过。每个成品附 SHA256 和 audit.json，记录实际架构、Git commit、Windows DLL 导入或 ELF INTERP/NEEDED 检查。只有审计确认无额外运行库时才发布。`BUILD-INFO.json` 标识 fork 的准确提交；基础 `--version` 沿用 Slumber 5.3.0。最终验证结果以该提交对应 Actions 记录为准，不将未完成的作业描述为通过。
