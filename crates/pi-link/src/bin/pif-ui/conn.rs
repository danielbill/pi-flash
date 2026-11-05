use super::*;

/// 一条已握手的连接；wait 复用同一条轮询。timeout = 全局 --timeout（单次
/// 请求读超时，call/wait 共用——此前 call 硬编码 15s，旗标只对握手生效）。
pub(super) struct Conn {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    pub(super) timeout: Duration,
}

impl Conn {
    pub(super) fn write_line(&mut self, line: &str) -> Result<(), String> {
        self.stream
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|e| format!("写连接失败: {e}"))
    }

    /// 读到指定 id 的 Response 为止（跳过服务端可能推送的其它记录）。
    pub(super) fn read_response(&mut self, id: &str, timeout: Duration) -> Result<Value, String> {
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| e.to_string())?;
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("读连接失败: {e}"))?;
            if n == 0 {
                return Err("连接已关闭（app 可能退出了）".into());
            }
            match automation::parse_line(&line) {
                Some(AutomationRecord::Response {
                    id: rid,
                    ok,
                    result,
                    error,
                }) if rid == id => {
                    return if ok {
                        Ok(result.unwrap_or_else(|| json!({})))
                    } else {
                        let e = error.unwrap_or(AutomationError::new("internal", ""));
                        Err(format!("服务端错误 [{}]: {}", e.code, e.message))
                    };
                }
                Some(_) => continue,
                None => continue, // 坏行忽略（与服务端同纪律）
            }
        }
    }

    pub(super) fn call(&mut self, m: &str, params: &Value) -> Result<Value, String> {
        let id = format!("cli-{}", now_ms());
        self.write_line(&automation::request_record(&id, m, params).to_string())?;
        self.read_response(&id, self.timeout)
    }
}

pub(super) fn handshake(stream: TcpStream, token: &str, timeout: Duration) -> Result<Conn, String> {
    stream.set_nodelay(true).ok();
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("设超时失败: {e}"))?;
    let reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut conn = Conn {
        stream,
        reader,
        timeout,
    };
    // hello
    let mut line = String::new();
    conn.reader
        .read_line(&mut line)
        .map_err(|e| format!("读 hello 失败: {e}"))?;
    match automation::parse_line(&line) {
        Some(AutomationRecord::Hello { proto, .. }) if proto == PROTO_VERSION => {}
        Some(AutomationRecord::Hello { proto, .. }) => {
            return Err(format!(
                "协议版本不匹配: 服务端 {proto}, CLI {PROTO_VERSION}"
            ));
        }
        _ => return Err("服务端首行不是 hello".into()),
    }
    conn.write_line(&automation::auth_record(token).to_string())?;
    let mut line = String::new();
    conn.reader
        .read_line(&mut line)
        .map_err(|e| format!("读 auth 应答失败: {e}"))?;
    match automation::parse_line(&line) {
        Some(AutomationRecord::AuthOk) => Ok(conn),
        Some(AutomationRecord::AuthError { code, message }) => {
            Err(format!("认证失败 [{code}]: {message}"))
        }
        _ => Err("服务端 auth 应答不可识别".into()),
    }
}

/// 找实例 → 连接 → 执行。选择序：--addr 直连 > PI_FLASH_AUTOMATION_FILE >
/// 实例目录里最新探活成功的一个（--pid 过滤）。
pub(super) fn with_conn(
    pid: Option<u32>,
    addr: Option<String>,
    token: Option<String>,
    timeout: u64,
    f: impl FnOnce(&mut Conn) -> Result<(), String>,
) -> Result<(), String> {
    let (addr, token) = match (addr, token) {
        (Some(a), t) => (a, t.ok_or("--addr 直连必须同时给 --token")?),
        _ => discover(pid)?,
    };
    let stream = TcpStream::connect(&addr).map_err(|e| format!("连不上 {addr}: {e}"))?;
    let mut conn = handshake(stream, &token, Duration::from_secs(timeout))?;
    f(&mut conn)
}

/// 判死规则见 automation::port_is_definitely_dead：只有明确拒绝连接才删
/// 登记，其它失败保留文件、直接当作候选返回（让真实连接去暴露真错）。
pub(super) fn discover(pid: Option<u32>) -> Result<(String, String), String> {
    if let Ok(f) = std::env::var("PI_FLASH_AUTOMATION_FILE") {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("读 {f}: {e}"))?;
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("解析 {f}: {e}"))?;
        return Ok((
            format!("127.0.0.1:{}", v["port"].as_u64().unwrap_or_default()),
            v["token"].as_str().unwrap_or_default().to_string(),
        ));
    }
    let candidates: Vec<(InstanceInfo, PathBuf)> = automation::read_instances()
        .into_iter()
        .filter(|(i, _)| pid.is_none_or(|p| i.pid == p))
        .collect();
    if candidates.is_empty() {
        return Err(match pid {
            Some(p) => {
                format!("没有 pid={p} 的自动化实例（app 需以 PI_FLASH_AUTOMATION=auto 启动）")
            }
            None => "没有找到自动化实例（app 需以 PI_FLASH_AUTOMATION=auto 启动）".into(),
        });
    }
    // 新的在前（read_instances 已排序）
    for (info, file) in candidates {
        let addr = format!("127.0.0.1:{}", info.port);
        if automation::port_is_definitely_dead(&addr) {
            let _ = std::fs::remove_file(&file); // 明确没人监听，登记是死的
            continue;
        }
        return Ok((addr, info.token));
    }
    Err("实例登记都在但端口明确拒绝连接（app 已退出？登记已清，重试或 pif-ui list）".into())
}
