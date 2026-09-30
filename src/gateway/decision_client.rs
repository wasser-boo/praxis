//! Bounded native /v1/decision client. No retries, model loading, tools, or DB
//! mutations. Every result is validated before a caller may apply any decision.
use super::decision_profiles::DecisionProfile;
use anyhow::{ensure,Context as _};
use serde_json::{json,Value};
use tokio_util::sync::CancellationToken;

#[derive(Debug,Clone)]
pub struct Decision { pub label:String, pub probability:f64 }

pub fn validate_results(profile:&DecisionProfile, data:&Value, count:usize)->anyhow::Result<Vec<Decision>> {
    let results=data["results"].as_array().context("Decision results missing")?;
    ensure!(results.len()==count,"Decision result count does not match contexts");
    let choices=profile.choices()?;
    results.iter().map(|result| {
        let label=result["decision"][&profile.state_field].as_str().context("Decision label missing")?;
        ensure!(choices.contains(&label),"Decision returned an undeclared choice");
        let field=&result["fields"][&profile.state_field];
        ensure!(field["value"].as_str()==Some(label),"Decision field and result disagree");
        let probability=field["probability"].as_f64().context("Decision probability missing")?;
        ensure!(probability.is_finite() && (0.0..=1.0).contains(&probability),"Invalid Decision probability");
        Ok(Decision {label:label.into(),probability})
    }).collect()
}

pub async fn decide(profile:&DecisionProfile, contexts:Vec<String>, cancel:&CancellationToken)->anyhow::Result<Vec<Decision>> {
    profile.validate()?;
    ensure!(!cancel.is_cancelled(),"Decision cancelled");
    ensure!(!contexts.is_empty() && contexts.len()<=256,"Decision expects 1–256 contexts");
    ensure!(contexts.iter().all(|s|s.chars().count()<=profile.max_context_chars),"Decision context exceeds configured bound");
    let count=contexts.len();
    let body=json!({"model":profile.model,"instructions":profile.instructions,"contexts":contexts,"schema":profile.schema,"mode":profile.mode,"cache_prompt":profile.cache_prompt});
    ensure!(body.to_string().len()<=1024*1024,"Decision request exceeds 1 MiB");
    // No credentials in profiles; no redirect may forward task data elsewhere.
    let client=reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build()?;
    let request=async {
        let mut response=client.post(&profile.endpoint).json(&body).send().await.context("Decision transport failed")?;
        ensure!(response.status().is_success(),"Decision HTTP {}",response.status().as_u16());
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await? {
            ensure!(bytes.len()+chunk.len()<=1024*1024,"Decision response exceeds 1 MiB");
            bytes.extend_from_slice(&chunk);
        }
        let data:Value=serde_json::from_slice(&bytes).context("Invalid Decision JSON")?;
        validate_results(profile,&data,count)
    };
    tokio::select! {
        biased;
        _=cancel.cancelled()=>anyhow::bail!("Decision cancelled"),
        result=tokio::time::timeout(std::time::Duration::from_millis(profile.timeout_ms),request)=>result.context("Decision timed out")?,
    }
}
