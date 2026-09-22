//! Fixed in-image verifier dispatcher. Check text never becomes a shell program.
use super::{config::WorkflowConfig, profile::RegisteredCommandProfile};
use crate::workflow::{TaskSpec, WorkflowSpec};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use serde_json::json;

// Each child has bounded output and a deadline. Its environment and argv are
// supplied solely by the server registry. Podman owns the entire process tree.
const DISPATCH: &str = r#"
import hashlib,json,os,selectors,signal,subprocess,sys,time
request=json.loads(sys.argv[1]); results=[]; total=0
os.mkdir('/workspace/.praxis-check-logs')
for index,check in enumerate(request['checks']):
    env=dict(os.environ); env.update(check['env']); output=hashlib.sha256(); code=126
    log=open('/workspace/.praxis-check-logs/'+str(index)+'.log','xb')
    try:
        p=subprocess.Popen([check['executable']]+check['argv'],cwd='/workspace',env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,start_new_session=True)
        selector=selectors.DefaultSelector(); selector.register(p.stdout,selectors.EVENT_READ); end=time.monotonic()+request['timeout']
        while selector.get_map():
            if time.monotonic()>end or total>=10*1024*1024:
                os.killpg(p.pid,signal.SIGKILL); code=124; break
            for key,_ in selector.select(0.1):
                data=os.read(key.fd,65536)
                if data:
                    data=data[:max(0,10*1024*1024-total)]; total+=len(data); output.update(data); log.write(data)
                    if total>=10*1024*1024: os.killpg(p.pid,signal.SIGKILL); code=124; break
                else: selector.unregister(key.fileobj)
        status=p.wait(); selector.close(); p.stdout.close()
        if code!=124: code=status if status>=0 else 128-status
    except Exception:
        code=126
    log.flush(); os.fsync(log.fileno()); log.close()
    results.append({'id':check['id'],'exit_code':code,'log_hash':output.hexdigest()})
print(json.dumps({'schema_version':1,'checks':results}),flush=True)
sys.exit(0 if all(r['exit_code']==0 for r in results) else 1)
"#;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckResult {
    pub id: String,
    pub exit_code: i32,
    pub log_hash: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Results {
    schema_version: u32,
    checks: Vec<CheckResult>,
}

pub fn command(
    config: &WorkflowConfig,
    spec: &WorkflowSpec,
    task: &TaskSpec,
) -> Result<RegisteredCommandProfile> {
    let mut checks = Vec::new();
    for check in &task.checks {
        let command = config.command(&check.profile_id, &spec.execution_profile_id)?;
        checks.push(json!({"id":check.id,"executable":command.executable,"argv":command.argv,"env":command.env}));
    }
    Ok(RegisteredCommandProfile {
        id: "workflow-verifier".into(),
        runtime_profile_id: spec.execution_profile_id.clone(),
        vendor_id: None,
        executable: config.verifier_executable.clone(),
        argv: vec![
            "-c".into(),
            DISPATCH.into(),
            json!({"checks":checks,"timeout":config.task_timeout_secs}).to_string(),
        ],
        env: Default::default(),
    })
}

pub fn parse(log: &str, task: &TaskSpec) -> Result<Vec<CheckResult>> {
    let results: Results = serde_json::from_str(log.trim()).context("invalid verifier receipt")?;
    ensure!(
        results.schema_version == 1 && results.checks.len() == task.checks.len(),
        "incomplete verifier receipt"
    );
    for (result, expected) in results.checks.iter().zip(&task.checks) {
        ensure!(
            result.id == expected.id
                && result.log_hash.len() == 64
                && result
                    .log_hash
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "verifier receipt identity mismatch"
        );
    }
    Ok(results.checks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dispatcher_retains_each_check_log_and_propagates_failure() {
        let root = std::env::temp_dir().join(format!("praxis-verifier-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let request = json!({"timeout":5,"checks":[
            {"id":"pass","executable":"/bin/echo","argv":["evidence"],"env":{}},
            {"id":"fail","executable":"/usr/bin/false","argv":[],"env":{}}
        ]});
        let output = std::process::Command::new("python3")
            .args([
                "-c",
                &DISPATCH.replace("/workspace", root.to_str().unwrap()),
                &request.to_string(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let results: Results = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(results.checks.len(), 2);
        assert_eq!(results.checks[0].exit_code, 0);
        assert_eq!(results.checks[1].exit_code, 1);
        for (index, result) in results.checks.iter().enumerate() {
            let bytes = std::fs::read(root.join(".praxis-check-logs").join(format!("{index}.log")))
                .unwrap();
            assert_eq!(result.log_hash, super::super::config::hash(&bytes));
        }
        assert_eq!(
            std::fs::read(root.join(".praxis-check-logs/0.log")).unwrap(),
            b"evidence\n"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn verifier_requires_every_declared_check_in_order() {
        let spec = WorkflowSpec::parse_json(include_str!(
            "../../../tests/fixtures/workflow-spec-v1.json"
        ))
        .unwrap();
        let task = spec
            .tasks
            .iter()
            .find(|t| t.id == spec.final_task_id)
            .unwrap();
        assert!(parse(r#"{"schema_version":1,"checks":[]}"#, task).is_err());
        let result = json!({"schema_version":1,"checks":task.checks.iter().map(|c|json!({"id":c.id,"exit_code":0,"log_hash":"a".repeat(64)})).collect::<Vec<_>>()});
        assert!(parse(&result.to_string(), task).is_ok());
    }
}
