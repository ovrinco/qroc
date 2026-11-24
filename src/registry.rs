//! Source-owned native repertoire. No model alias or registry entry grants effects.
use crate::{Error, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
pub const VERSION: &str = "qroc-native-registry/1";
/// Product is the working pipeline; later advantages keep this order when triggered.
pub const DOMAIN_ORDER: [&str; 7] = [
    "PRODUCT",
    "DISTRIBUTION",
    "ECOSYSTEM",
    "BRAND",
    "DATA",
    "COMMUNITY",
    "CAPITAL",
];

/// Read legacy project identities without rewriting their durable records.
pub fn canonical_domain(domain: &str) -> &str {
    match domain {
        "BUSINESS" => "DISTRIBUTION",
        "PRESENCE" => "BRAND",
        "LEGAL" => "ECOSYSTEM",
        other => other,
    }
}

pub fn domain_purpose(domain: &str) -> &'static str {
    match canonical_domain(domain) {
        "PRODUCT" => "Turn the idea into a specified product and its documents.",
        "DISTRIBUTION" => "Find customers and bring the product to them.",
        "ECOSYSTEM" => "Build the relationships and agreements the product depends on.",
        "BRAND" => "Give the product a clear identity people can recognise.",
        "DATA" => "Use information responsibly to improve the product.",
        "COMMUNITY" => "Bring people together around the product.",
        "CAPITAL" => "Plan the money needed to sustain and grow the product.",
        _ => "Unknown domain.",
    }
}

/// Each domain's own folder in a project's readable folder. Product, Business, Presence and
/// Legal keep the names they have always had on disk; Data, Community and Capital have their own.
pub fn domain_folder(domain: &str) -> &'static str {
    match canonical_domain(domain) {
        "PRODUCT" => "product",
        "DISTRIBUTION" => "business",
        "ECOSYSTEM" => "legal",
        "BRAND" => "presence",
        "DATA" => "data",
        "COMMUNITY" => "community",
        "CAPITAL" => "capital",
        _ => "product",
    }
}

/// Existing document types keep their storage keys and acquire an advantage domain.
pub fn document_domain(kind: &str) -> &'static str {
    match kind {
        "MARKET_ANALYSIS"
        | "COMPETITOR_ANALYSIS"
        | "BUSINESS_PLAN"
        | "GO_TO_MARKET"
        | "OUTBOUND_STRATEGY"
        | "INBOUND_STRATEGY"
        | "ADS_STRATEGY"
        | "GATE_4"
        | "GATE_5" => "DISTRIBUTION",
        "BRAND_GUIDE"
        | "MESSAGING_FRAMEWORK"
        | "WEBSITE_SOCIAL"
        | "VISUAL_ASSETS"
        | "GATE_7"
        | "GATE_8" => "BRAND",
        "FINANCIAL_OVERVIEW" | "SLIDE_DECK" | "FINANCIAL_SENSITIVITY" | "GATE_6" => "CAPITAL",
        "COMPLIANCE_PRIVACY" | "GATE_9" => "DATA",
        "PR_POLICY" => "COMMUNITY",
        "LICENSING" | "TERMS" | "CONTRACTS" | "REGISTRY_PREP" | "GATE_10" => "ECOSYSTEM",
        _ => "PRODUCT",
    }
}

/// Retain all predecessor gate checks from the legacy groups. The later data and
/// community triggers reuse their existing legal and presence review evidence.
pub fn domain_gates(domain: &str) -> &'static [&'static str] {
    match canonical_domain(domain) {
        "PRODUCT" => &["GATE_1", "GATE_2", "GATE_3"],
        "DISTRIBUTION" => &["GATE_4", "GATE_5", "GATE_6"],
        "ECOSYSTEM" | "DATA" => &["GATE_9", "GATE_10"],
        "BRAND" | "COMMUNITY" => &["GATE_7", "GATE_8"],
        _ => &[],
    }
}
pub struct RegistryStore {
    root: PathBuf,
}
impl RegistryStore {
    pub fn open(root: &Path) -> Result<Self> {
        crate::scope::validate_root(root)?;
        Ok(Self { root: root.into() })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn command(&mut self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "qroc-registry-catalog" => Ok(
                json!({"version":VERSION,"source":"src/qroc.rs","types":catalog()?,"semantic_force_registry":{"available":true,"supported_profiles":[SEMANTIC_PROFILE],"complete_catalogue_available":false,"reason":"Bounded source-backed market review only; other semantic profiles unavailable"},"execution_authority":false}),
            ),
            "qroc-registry-type" => {
                let kind = args["document_type"].as_str().unwrap_or("");
                Ok(entry(kind)?)
            }
            "qroc-registry-semantic-profile" => {
                if args
                    .get("profile")
                    .is_some_and(|v| v.as_str() != Some(SEMANTIC_PROFILE))
                {
                    return Err(invalid("capability-unavailable: unknown semantic profile"));
                }
                Ok(
                    json!({"version":SEMANTIC_VERSION,"profile":semantic_profile()?,"profile_canonical_json":PROFILE_JSON,"profile_sha256":profile_sha256(),"sources":semantic_sources(),"validated":true,"execution_authority":false}),
                )
            }
            "qroc-registry-semantic-validate" => {
                validate_profile(&args["profile"])?;
                Ok(
                    json!({"validated":true,"profile_id":args["profile"]["id"],"standing":"structural validation only; caller registry is not installed or admitted","execution_authority":false}),
                )
            }
            "qroc-registry-provider-capabilities" => Ok(provider_capabilities_at(
                self.root.parent().map(|p| p.join("qroc")).as_deref(),
            )),
            _ => Err(Error::Invalid(
                "capability-unavailable: registry adapter is not implemented".into(),
            )),
        }
    }
}
pub fn catalog() -> Result<Vec<Value>> {
    crate::qroc::document_order_map()
        .as_object()
        .unwrap()
        .values()
        .flat_map(|v| v.as_array().unwrap())
        .map(|v| entry(v.as_str().unwrap()))
        .collect()
}
pub fn entry(kind: &str) -> Result<Value> {
    let kind = crate::qroc::normalized_document(kind)?;
    let order = crate::qroc::document_order(&kind);
    let upstream = product_inputs(&kind)
        .map(|v| v.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        .unwrap_or_else(|| {
            catalog_types()
                .into_iter()
                .filter(|k| crate::qroc::document_order(k) < order)
                .collect::<Vec<_>>()
        });
    let mut record = json!({"id":kind,"domain":document_domain(&kind),"order":order,"source":"src/qroc.rs:qroc_prompt/document_template","available":true,"input_policy":{"stage":if order==0 {"first"} else {"middle"},"allowed_upstream":upstream,"forbid_forward_references":true,"synthesis_only":false},"output_contract":if kind.starts_with("GATE_"){"native-score-criteria-review"}else{"native-document-draft"},"gate_threshold":if kind=="GATE_2"{Some(92)}else if kind.starts_with("GATE_"){Some(90)}else{None},"semantic_force_profile_available":false});
    if supports_semantic(&kind) {
        let p = semantic_profile()?;
        let model = p["documents"]
            .as_array()
            .unwrap()
            .iter()
            .chain(p["gates"].as_array().unwrap())
            .find(|m| m["documentType"] == kind || m["gateType"] == kind)
            .unwrap();
        record["semantic_force_profile_available"] = json!(true);
        record["semantic_profile"] = json!(SEMANTIC_PROFILE);
        record["semantic_model"] = model.clone();
    }
    Ok(record)
}
pub fn catalog_types() -> Vec<String> {
    crate::qroc::document_order_map()
        .as_object()
        .unwrap()
        .values()
        .flat_map(|v| v.as_array().unwrap())
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}
/// Discovery never connects or starts a provider. Identity is an explicit local declaration.
pub fn provider_capabilities() -> Value {
    let provider = provider_name().unwrap_or_default();
    match provider_config() {
        Ok((endpoint, model)) => {
            json!({"available":true,"provider":provider,"endpoint":endpoint,"model":model,"standing":"explicitly-configured; reachability/inference unverified"})
        }
        Err(e) => json!({"available":false,"provider":provider,"reason":e.to_string()}),
    }
}
/// The route generation uses for the store at `root`: the routes table's selected route.
/// Launch variables only seed: they stand in while no table or no selection exists.
pub(crate) fn provider_name_at(root: &Path) -> Result<String> {
    match crate::routes::selected_at(root)? {
        Some(r) => Ok(r.name),
        None => provider_name(),
    }
}
pub(crate) fn provider_config_at(root: &Path) -> Result<(String, String)> {
    match crate::routes::selected_at(root)? {
        Some(r) => Ok((r.endpoint, r.model)),
        None => provider_config(),
    }
}
/// Capabilities as the store at `root` would generate; without a store, the launch variables.
pub fn provider_capabilities_at(root: Option<&Path>) -> Value {
    let Some(root) = root else {
        return provider_capabilities();
    };
    let provider = provider_name_at(root).unwrap_or_default();
    if let Ok(Some(r)) = crate::routes::selected_at(root)
        && crate::routes::key_missing(&r, &crate::routes::env_var)
    {
        return json!({"available":false,"provider":provider,"needs_key":true,"reason":format!("capability-unavailable: {}", crate::routes::NEEDS_KEY)});
    }
    match provider_config_at(root) {
        Ok((endpoint, model)) => {
            json!({"available":true,"provider":provider,"endpoint":endpoint,"model":model,"standing":"explicitly-configured; reachability/inference unverified"})
        }
        Err(e) => json!({"available":false,"provider":provider,"reason":e.to_string()}),
    }
}
pub(crate) fn provider_name() -> Result<String> {
    let name = std::env::var("QROC_PROVIDER").unwrap_or_else(|_| "local-gateway".into());
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(Error::Invalid(
            "capability-unavailable: invalid QROC_PROVIDER (expected [a-z0-9-]{1,64})".into(),
        ));
    }
    Ok(name)
}
pub(crate) fn provider_config() -> Result<(String, String)> {
    provider_name()?;
    let endpoint = std::env::var("QROC_GATEWAY").map_err(|_| {
        Error::Invalid("capability-unavailable: QROC_GATEWAY is not declared".into())
    })?;
    crate::qroc::parse_local_endpoint(&endpoint)?;
    let model = std::env::var("QROC_MODEL")
        .map_err(|_| Error::Invalid("capability-unavailable: QROC_MODEL is not declared".into()))?;
    if model.trim().is_empty() || model.len() > 200 {
        return Err(Error::Invalid("Invalid declared QROC_MODEL".into()));
    }
    Ok((endpoint, model))
}

const PROFILE_JSON: &str = r###"{
  "id": "retained-market-review/1",
  "documents": [
    {
      "documentType": "MARKET_ANALYSIS",
      "name": "Market Analysis",
      "question": "How big is this opportunity, and how do we know?",
      "domain": "BUSINESS",
      "inputPolicy": {
        "stage": "first",
        "allowedUpstream": [
          "STRATEGY_BRIEF",
          "PRD",
          "TECHNICAL_ARCHITECTURE"
        ],
        "forbidForwardReferences": true
      },
      "forces": [
        {
          "id": "market_analysis.tam_sam_som_math",
          "description": "TAM/SAM/SOM must be CALCULATED, not asserted. State methodology: top-down (industry data), bottom-up (customers x price), or analog (comparable company extrapolation). Show the arithmetic",
          "priority": 1,
          "tags": [
            "market-sizing",
            "math"
          ]
        },
        {
          "id": "market_analysis.segment_specificity",
          "description": "Customer segments must be people a salesperson would recognize, not demographic abstractions. \"Mid-market SaaS companies with 50-500 employees struggling with documentation\" is a segment. \"Technology professionals aged 25-45\" is demographics",
          "priority": 1,
          "tags": [
            "segmentation"
          ]
        },
        {
          "id": "market_analysis.dated_sources",
          "description": "Data must be dated and sourced. A 2021 market report in 2026 is stale. State the date and the source, or state \"estimated based on [reasoning]\"",
          "priority": 2,
          "tags": [
            "evidence",
            "freshness"
          ]
        },
        {
          "id": "market_analysis.som_conservatism",
          "description": "SOM must be conservative. If SOM is > 5% of SAM in year 1 for a new entrant, the analysis is not credible",
          "priority": 2,
          "tags": [
            "sizing",
            "credibility"
          ]
        },
        {
          "id": "market_analysis.timing_window",
          "description": "Timing matters: why NOW? What changed in the last 2-3 years that creates this window? What happens if entry is delayed?",
          "priority": 3,
          "tags": [
            "timing"
          ]
        }
      ],
      "failureModes": [
        {
          "id": "market_analysis.headline_tam",
          "description": "TAM from a headline report without verifying relevance to THIS product"
        },
        {
          "id": "market_analysis.sam_not_subset",
          "description": "SAM that's not a proper subset of TAM (different geographies or segments)"
        },
        {
          "id": "market_analysis.demographic_segments",
          "description": "Customer segments that are just demographics without buying behavior or switching triggers"
        },
        {
          "id": "market_analysis.headwinds_ignored",
          "description": "Ignoring market headwinds (regulation tightening, incumbents consolidating, technology maturing)"
        },
        {
          "id": "market_analysis.timing_handwave",
          "description": "Market timing claim that's just \"AI is hot right now\""
        }
      ],
      "readerTest": "Someone working in the target market reads this and thinks: \"They understand my world. That's my problem. That's what I'd pay for.\"",
      "projectTypeAdaptation": {
        "SaaS": {
          "guidance": [
            "Focus on expansion dynamics within existing accounts."
          ]
        },
        "Marketplace": {
          "guidance": [
            "Analyze supply-side and demand-side separately, with different segments and sizing."
          ]
        },
        "DevTool": {
          "guidance": [
            "Size by developer population and adoption probability instead of enterprise revenue."
          ]
        },
        "ConsumerApp": {
          "guidance": [
            "Use behavioral cohort analysis instead of demographic slices alone."
          ]
        },
        "Enterprise": {
          "guidance": [
            "Size by deal value, addressable accounts, and realistic win rate."
          ]
        }
      },
      "outputContract": {
        "mode": "strict-doc",
        "requiredSections": [
          "Sizing Methodology",
          "Customer Segments",
          "TAM / SAM / SOM",
          "Market Timing",
          "Headwinds and Risks"
        ]
      },
      "structuralGuidance": "This is the first Business document. Use Product-domain context (Strategy Brief, PRD, TAD) and the brain dump to build the market analysis from scratch. Show arithmetic, state source dates and whether each source is primary or secondary, include switching triggers for each segment, and explain what happens if entry is delayed by 12 months."
    },
    {
      "documentType": "COMPETITOR_ANALYSIS",
      "name": "Competitor Analysis",
      "question": "Can this business survive contact with the market?",
      "domain": "BUSINESS",
      "inputPolicy": {
        "stage": "middle",
        "allowedUpstream": [
          "STRATEGY_BRIEF",
          "PRD",
          "TECHNICAL_ARCHITECTURE",
          "MARKET_ANALYSIS"
        ],
        "forbidForwardReferences": true
      },
      "forces": [
        {
          "id": "competitor_analysis.honest_competitor_strengths",
          "description": "Honesty about competitor strengths is a credibility signal. Underestimating competitors is strategic malpractice. A reader who knows the space will immediately detect dishonesty",
          "priority": 1,
          "tags": [
            "credibility",
            "honesty"
          ]
        },
        {
          "id": "competitor_analysis.structural_differentiation",
          "description": "Differentiation must be STRUCTURAL, not feature-based. Features can be copied. Network effects, data moats, switching costs, regulatory advantages cannot",
          "priority": 1,
          "upstreamReference": "TECHNICAL_ARCHITECTURE",
          "crossDomain": true,
          "tags": [
            "moat",
            "differentiation"
          ]
        },
        {
          "id": "competitor_analysis.threat_categories",
          "description": "Must consider three threat categories: direct competitors (same problem, same audience), indirect competitors (same audience, different approach), and the status quo (people doing nothing, which is always the strongest competitor for new categories)",
          "priority": 2,
          "tags": [
            "competition",
            "status-quo"
          ]
        },
        {
          "id": "competitor_analysis.response_scenarios",
          "description": "Competitive response scenarios matter more than current positioning: if we succeed, what do incumbents do? Can they copy us? Can they acquire us? Can they cut prices to zero?",
          "priority": 2,
          "tags": [
            "strategy",
            "response"
          ]
        },
        {
          "id": "competitor_analysis.project_type_landscape",
          "description": "Project type affects the competitive landscape. Open-source tools compete with other open-source AND proprietary. Marketplaces compete with direct transactions",
          "priority": 3,
          "tags": [
            "project-type"
          ]
        }
      ],
      "failureModes": [
        {
          "id": "competitor_analysis.stacked_feature_table",
          "description": "Feature comparison table where every checkmark favors us"
        },
        {
          "id": "competitor_analysis.no_competitors",
          "description": "\"No direct competitors\" (there are always competitors — at minimum, the status quo)"
        },
        {
          "id": "competitor_analysis.invented_advantages",
          "description": "Claiming advantages the product doesn't actually have (reference the TAD)"
        },
        {
          "id": "competitor_analysis.dismiss_big_players",
          "description": "Dismissing large competitors as \"slow\" or \"legacy\" without evidence"
        },
        {
          "id": "competitor_analysis.rigged_matrix",
          "description": "Positioning matrix dimensions chosen to make us look good rather than reflecting buyer priorities"
        }
      ],
      "readerTest": "A prospective customer would trust this analysis because it's honest about what alternatives do well, not just what they do badly.",
      "projectTypeAdaptation": {
        "SaaS": {
          "guidance": [
            "Focus on switching costs, migration difficulty, and integration depth."
          ]
        },
        "OpenSource": {
          "guidance": [
            "Compare against both OSS and proprietary competitors; the dynamics are different."
          ]
        },
        "Marketplace": {
          "guidance": [
            "Treat network effects as the moat and analyze liquidity thresholds and cold-start paths."
          ]
        },
        "Enterprise": {
          "guidance": [
            "Account for incumbent relationships, procurement inertia, and compliance advantages."
          ]
        },
        "Hardware": {
          "guidance": [
            "Look for supply chain advantages, manufacturing IP, and certification barriers."
          ]
        }
      },
      "outputContract": {
        "mode": "strict-doc",
        "requiredSections": [
          "Competitive Landscape",
          "Threat Categories",
          "Structural Moats",
          "Response Scenarios",
          "Strategic Implications"
        ]
      },
      "structuralGuidance": "Use the Market Analysis landscape data and reference specific competitors instead of categories. Evaluate direct, indirect, and status-quo competitors, and make the moat analysis trace to actual product capabilities from the TAD."
    }
  ],
  "gates": [
    {
      "gateType": "GATE_4",
      "name": "Market Research Review",
      "domain": "BUSINESS",
      "threshold": 90,
      "reviewedDocuments": [
        "MARKET_ANALYSIS",
        "COMPETITOR_ANALYSIS"
      ],
      "criteria": [
        {
          "id": "gate_4.market_analysis_rigor",
          "name": "Market Analysis Rigor",
          "weight": 25,
          "checks": [
            "Is TAM calculated from a stated methodology with arithmetic shown?",
            "Are SAM and SOM realistic subsets rather than inflated?",
            "Are customer segments specific enough for a salesperson to identify targets?",
            "Is timing analysis present and credible?"
          ],
          "derivedFromForces": [
            {
              "documentType": "MARKET_ANALYSIS",
              "forceId": "market_analysis.tam_sam_som_math"
            },
            {
              "documentType": "MARKET_ANALYSIS",
              "forceId": "market_analysis.segment_specificity"
            },
            {
              "documentType": "MARKET_ANALYSIS",
              "forceId": "market_analysis.dated_sources"
            },
            {
              "documentType": "MARKET_ANALYSIS",
              "forceId": "market_analysis.som_conservatism"
            }
          ]
        },
        {
          "id": "gate_4.competitive_positioning",
          "name": "Competitive Positioning",
          "weight": 25,
          "checks": [
            "Is differentiation structural rather than feature-based?",
            "Are competitor strengths acknowledged honestly?",
            "Are direct, indirect, and status-quo threats all addressed?",
            "Are competitive response scenarios considered?"
          ],
          "derivedFromForces": [
            {
              "documentType": "COMPETITOR_ANALYSIS",
              "forceId": "competitor_analysis.honest_competitor_strengths"
            },
            {
              "documentType": "COMPETITOR_ANALYSIS",
              "forceId": "competitor_analysis.structural_differentiation"
            },
            {
              "documentType": "COMPETITOR_ANALYSIS",
              "forceId": "competitor_analysis.threat_categories"
            },
            {
              "documentType": "COMPETITOR_ANALYSIS",
              "forceId": "competitor_analysis.response_scenarios"
            }
          ]
        },
        {
          "id": "gate_4.market_competitive_consistency",
          "name": "Market-Competitive Consistency",
          "weight": 25,
          "checks": [
            "Do market segments align with the competitive landscape analysis?",
            "Does competitive positioning align with the differentiation strategy?",
            "Are the same customer segments referenced consistently across both documents?"
          ],
          "derivedFromForces": [
            {
              "documentType": "MARKET_ANALYSIS",
              "forceId": "market_analysis.segment_specificity"
            },
            {
              "documentType": "COMPETITOR_ANALYSIS",
              "forceId": "competitor_analysis.structural_differentiation"
            },
            {
              "documentType": "COMPETITOR_ANALYSIS",
              "forceId": "competitor_analysis.project_type_landscape"
            }
          ]
        },
        {
          "id": "gate_4.evidence_quality",
          "name": "Evidence Quality",
          "weight": 25,
          "checks": [
            "Are data sources dated and credible?",
            "Are market timing claims substantiated beyond hype?",
            "Is there a \"what if we're wrong\" consideration for the top assumptions?"
          ],
          "derivedFromForces": [
            {
              "documentType": "MARKET_ANALYSIS",
              "forceId": "market_analysis.dated_sources"
            },
            {
              "documentType": "MARKET_ANALYSIS",
              "forceId": "market_analysis.timing_window"
            },
            {
              "documentType": "COMPETITOR_ANALYSIS",
              "forceId": "competitor_analysis.honest_competitor_strengths"
            }
          ]
        }
      ],
      "consistencyRules": [
        {
          "id": "gate_4.segment_competitive_alignment",
          "kind": "force_alignment",
          "severity": 2,
          "left": {
            "documentType": "MARKET_ANALYSIS",
            "forceId": "market_analysis.segment_specificity"
          },
          "right": {
            "documentType": "COMPETITOR_ANALYSIS",
            "forceId": "competitor_analysis.threat_categories"
          },
          "expectation": "consistent",
          "message": "Customer segments in Market Analysis must align with the competitive threat categories — competitors should map to the same segments being sized."
        },
        {
          "id": "gate_4.landscape_consistency",
          "kind": "force_alignment",
          "severity": 2,
          "left": {
            "documentType": "MARKET_ANALYSIS",
            "forceId": "market_analysis.som_conservatism"
          },
          "right": {
            "documentType": "COMPETITOR_ANALYSIS",
            "forceId": "competitor_analysis.project_type_landscape"
          },
          "expectation": "same_scope",
          "message": "Market sizing assumptions and competitive landscape must describe the same category dynamics and addressable scope."
        }
      ],
      "outputContract": {
        "mode": "gate-review",
        "requiresScoreTag": true,
        "requiresMachineReadableCriteria": true,
        "requiredSections": [
          "CRITERIA BREAKDOWN",
          "CRITICAL ISSUES",
          "VERDICT"
        ]
      }
    }
  ],
  "externalInputs": [
    "STRATEGY_BRIEF",
    "PRD",
    "TECHNICAL_ARCHITECTURE"
  ],
  "documentOrder": [
    "MARKET_ANALYSIS",
    "COMPETITOR_ANALYSIS",
    "GATE_4"
  ]
}
"###;
const SEMANTIC_SOURCES_JSON: &str = r###"{"/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/domain_specification.md":"9b5149edcb071ac2c429e4b54604cab844026f40645a914366dcba94586dfeef","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/domain/documentModels/business.ts":"0d8c528febb50489774ca0c1125c8c3aa38a84ba70beafcae889793344327a76","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/domain/documentModels/helpers.ts":"2d3251d0b0d040a6d56cf812b19f59f18bfb16d2e05d86cd960c4b5aac89c365","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/domain/documentModels/registry.ts":"cf5710f5ec006afdfae1d35ba8342cc9d49d6e1a733e54cdb5766de32ec75bf2","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/domain/documentModels/types.ts":"19caf36c27bf8f0c8b084c62a641a90b41499c960d58b5f1735e54caf83c04c8","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/services/gateCompiler.ts":"c77508ef12a0c62e24d6684c3c217fabeb58eecaf10bc79094b03c0c893b3b46","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/services/gateEngine.ts":"6bce8c188ab60b1879943725fa609e3bdadad3472139404c80417b384631e2ab","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/services/gateParser.ts":"bc00f2c22ed1c45016d07ffdb2c4b7ae7496e44b53f69e91124a058934220a21","/example/operator/04_memory/runs/qroc/2026-10-01_standalone_campaign/inputs/salvage/react/services/promptCompiler.ts":"452878d313f21b2440b5c23c1f4db1a187f829dd09202f4d3c6d27571ef7563d"}"###;

pub const SEMANTIC_VERSION: &str = "qroc-semantic-registry/1";
pub const SEMANTIC_PROFILE: &str = "retained-market-review/1";
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn require(ok: bool, message: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(invalid(message)) }
}
fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array()
        .ok_or_else(|| invalid("Required semantic array missing"))
}
fn text(v: &Value) -> Result<&str> {
    v.as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("Required semantic text missing"))
}
fn number(v: &Value) -> Result<u64> {
    v.as_u64()
        .ok_or_else(|| invalid("Required semantic non-negative integer missing"))
}
fn unique_strings(v: &Value) -> Result<BTreeSet<String>> {
    let mut set = BTreeSet::new();
    for item in array(v)? {
        require(
            set.insert(text(item)?.into()),
            "Duplicate semantic identity",
        )?;
    }
    Ok(set)
}
pub fn supports_semantic(kind: &str) -> bool {
    matches!(kind, "MARKET_ANALYSIS" | "COMPETITOR_ANALYSIS" | "GATE_4")
}
pub fn profile_sha256() -> String {
    format!("{:x}", Sha256::digest(PROFILE_JSON.as_bytes()))
}
pub fn semantic_profile() -> Result<Value> {
    let v: Value = serde_json::from_str(PROFILE_JSON)?;
    validate_profile(&v)?;
    Ok(v)
}
pub fn semantic_sources() -> Value {
    serde_json::from_str(SEMANTIC_SOURCES_JSON).expect("compiled source identity JSON")
}
/// A caller registry may be checked, but can never replace the admitted source model.
pub fn validate_profile(profile: &Value) -> Result<()> {
    require(
        serde_json::to_vec(profile)?.len() <= 512 * 1024,
        "Semantic registry exceeds 512 KiB",
    )?;
    text(&profile["id"])?;
    let docs = array(&profile["documents"])?;
    let gates = array(&profile["gates"])?;
    require(
        !docs.is_empty() && docs.len() <= 64 && !gates.is_empty() && gates.len() <= 32,
        "Semantic registry model bounds invalid",
    )?;
    let external = unique_strings(&profile["externalInputs"])?;
    for id in &external {
        crate::qroc::normalized_document(id)?;
        require(
            !id.starts_with("GATE_"),
            "External gate prerequisite unsupported",
        )?;
    }
    let mut models = BTreeMap::new();
    let mut forces = BTreeSet::new();
    let mut failures = BTreeSet::new();
    let mut all_ids = external.clone();
    for d in docs {
        let id = text(&d["documentType"])?;
        require(
            crate::qroc::normalized_document(id)? == id && !id.starts_with("GATE_"),
            "Unknown/noncanonical semantic document",
        )?;
        require(
            all_ids.insert(id.into()) && models.insert(id.to_string(), d).is_none(),
            "Duplicate document/external identity",
        )?;
        require(
            canonical_domain(text(&d["domain"])?) == canonical_domain(crate::qroc::domain_for(id)),
            "Semantic document domain mismatch",
        )?;
        for field in ["name", "question", "readerTest"] {
            text(&d[field])?;
        }
        require(
            matches!(
                text(&d["outputContract"]["mode"])?,
                "strict-doc" | "flex-doc"
            ),
            "Unknown document output contract",
        )?;
        require(
            !array(&d["outputContract"]["requiredSections"])?.is_empty(),
            "Missing required document output sections",
        )?;
        unique_strings(&d["outputContract"]["requiredSections"])?;
        let fs = array(&d["forces"])?;
        require(
            !fs.is_empty() && fs.len() <= 64,
            "Missing/too many semantic forces",
        )?;
        for f in fs {
            let fid = text(&f["id"])?;
            require(
                fid.starts_with(&format!("{}.", id.to_ascii_lowercase()))
                    && forces.insert((id.to_string(), fid.to_string())),
                "Duplicate/invalid force identity",
            )?;
            text(&f["description"])?;
            require(
                (1..=3).contains(&number(&f["priority"])?),
                "Invalid force priority",
            )?;
        }
        for f in array(&d["failureModes"])? {
            require(
                failures.insert(text(&f["id"])?.to_string()),
                "Duplicate failure identity",
            )?;
            text(&f["description"])?;
        }
    }
    let order = array(&profile["documentOrder"])?;
    let mut positions = BTreeMap::new();
    for (i, v) in order.iter().enumerate() {
        require(
            positions.insert(text(v)?.to_string(), i).is_none(),
            "Duplicate ordered type",
        )?;
    }
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (id, d) in &models {
        let policy = &d["inputPolicy"];
        let stage = text(&policy["stage"])?;
        require(
            matches!(stage, "first" | "middle" | "terminal" | "singleton"),
            "Unknown semantic input stage",
        )?;
        require(
            policy["forbidForwardReferences"] == true,
            "Forward references may not be enabled",
        )?;
        if let Some(v) = policy.get("synthesisOnly") {
            require(v.is_boolean(), "Invalid synthesis-only policy")?;
        }
        let upstream = unique_strings(&policy["allowedUpstream"])?;
        for u in &upstream {
            require(all_ids.contains(u), "Unknown required upstream type")?;
        }
        if matches!(stage, "first" | "singleton") {
            require(
                upstream.iter().all(|u| external.contains(u)),
                "First/singleton stage has internal upstream",
            )?;
        }
        edges.insert(
            id.clone(),
            upstream
                .iter()
                .filter(|u| models.contains_key(*u))
                .cloned()
                .collect(),
        );
        for f in array(&d["forces"])? {
            if let Some(u) = f.get("upstreamReference") {
                require(
                    upstream.contains(text(u)?),
                    "Force upstream outside declared input policy",
                )?;
            }
        }
    }
    fn visit(
        id: &str,
        edges: &BTreeMap<String, BTreeSet<String>>,
        active: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        require(
            active.insert(id.into()),
            "Circular semantic input dependency",
        )?;
        if let Some(next) = edges.get(id) {
            for u in next {
                visit(u, edges, active, done)?;
            }
        }
        active.remove(id);
        done.insert(id.into());
        Ok(())
    }
    let mut done = BTreeSet::new();
    for id in models.keys() {
        visit(id, &edges, &mut BTreeSet::new(), &mut done)?;
    }
    for (id, d) in &models {
        let pos = positions
            .get(id)
            .ok_or_else(|| invalid("Document missing from semantic order"))?;
        for u in unique_strings(&d["inputPolicy"]["allowedUpstream"])? {
            if let Some(up) = positions.get(&u) {
                require(up < pos, "Forward/impossible semantic upstream order")?;
            } else {
                require(external.contains(&u), "Missing upstream order")?;
            }
            require(
                crate::qroc::document_order(&u) < crate::qroc::document_order(id),
                "Forward native upstream order",
            )?;
        }
    }
    let mut criteria = BTreeSet::new();
    let mut rules = BTreeSet::new();
    let resolve = |r: &Value| -> Result<()> {
        require(
            forces.contains(&(
                text(&r["documentType"])?.into(),
                text(&r["forceId"])?.into(),
            )),
            "Unknown force reference",
        )
    };
    for g in gates {
        let id = text(&g["gateType"])?;
        require(
            crate::qroc::normalized_document(id)? == id
                && id.starts_with("GATE_")
                && all_ids.insert(id.into()),
            "Unknown/duplicate gate identity",
        )?;
        require(
            canonical_domain(text(&g["domain"])?) == canonical_domain(crate::qroc::domain_for(id)),
            "Gate domain mismatch",
        )?;
        text(&g["name"])?;
        require(
            (1..=100).contains(&number(&g["threshold"])?),
            "Invalid gate threshold",
        )?;
        let output = &g["outputContract"];
        require(
            output["mode"] == "gate-review"
                && output["requiresScoreTag"] == true
                && output["requiresMachineReadableCriteria"] == true,
            "Invalid required gate output contract",
        )?;
        require(
            !unique_strings(&output["requiredSections"])?.is_empty(),
            "Missing gate sections",
        )?;
        let reviewed = unique_strings(&g["reviewedDocuments"])?;
        require(!reviewed.is_empty(), "Missing reviewed documents")?;
        let pos = positions
            .get(id)
            .ok_or_else(|| invalid("Gate missing from order"))?;
        for d in &reviewed {
            require(models.contains_key(d), "Unknown gate reviewed document")?;
            require(
                positions.get(d).is_some_and(|p| p < pos),
                "Impossible gate upstream order",
            )?;
        }
        let cs = array(&g["criteria"])?;
        require(
            !cs.is_empty() && cs.len() <= 64,
            "Missing required gate criteria",
        )?;
        let mut sum = 0;
        for c in cs {
            let cid = text(&c["id"])?;
            require(
                cid.starts_with(&format!("{}.", id.to_ascii_lowercase()))
                    && criteria.insert(cid.to_string()),
                "Duplicate/invalid criterion identity",
            )?;
            text(&c["name"])?;
            let weight = number(&c["weight"])?;
            require((1..=100).contains(&weight), "Invalid criterion weight")?;
            sum += weight;
            require(
                !unique_strings(&c["checks"])?.is_empty(),
                "Missing criterion checks",
            )?;
            let refs = array(&c["derivedFromForces"])?;
            require(!refs.is_empty(), "Missing criterion force refs")?;
            let mut seen = BTreeSet::new();
            for r in refs {
                resolve(r)?;
                require(
                    reviewed.contains(text(&r["documentType"])?)
                        && seen.insert(serde_json::to_string(r)?),
                    "Duplicate/unreviewed criterion force ref",
                )?;
            }
        }
        require(sum == 100, "Criterion weights must total 100")?;
        for r in array(&g["consistencyRules"])? {
            let rid = text(&r["id"])?;
            require(
                rid.starts_with(&format!("{}.", id.to_ascii_lowercase()))
                    && rules.insert(rid.to_string()),
                "Duplicate/invalid consistency rule",
            )?;
            require(
                r["kind"] == "force_alignment",
                "Unsupported required consistency rule kind",
            )?;
            require(
                matches!(
                    text(&r["expectation"])?,
                    "consistent" | "supports" | "same_scope"
                ),
                "Unknown consistency expectation",
            )?;
            require(
                (1..=3).contains(&number(&r["severity"])?),
                "Invalid rule severity",
            )?;
            text(&r["message"])?;
            for f in [&r["left"], &r["right"]] {
                resolve(f)?;
                require(
                    reviewed.contains(text(&f["documentType"])?),
                    "Unreviewed rule force",
                )?;
            }
        }
    }
    let owned = all_ids
        .difference(&external)
        .cloned()
        .collect::<BTreeSet<_>>();
    require(
        positions.keys().cloned().collect::<BTreeSet<_>>() == owned,
        "Unknown/missing ordered model",
    )?;
    Ok(())
}

/// Strict recursive JSON parsing rejects duplicate object keys rather than last-key wins.
pub fn parse_unique_json(input: &str) -> Result<Value> {
    struct Unique;
    impl<'de> serde::de::DeserializeSeed<'de> for Unique {
        type Value = Value;
        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            d: D,
        ) -> std::result::Result<Value, D::Error> {
            d.deserialize_any(self)
        }
    }
    impl<'de> serde::de::Visitor<'de> for Unique {
        type Value = Value;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("JSON without duplicate keys")
        }
        fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Value, E> {
            Ok(json!(v))
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Value, E> {
            Ok(json!(v))
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Value, E> {
            Ok(json!(v))
        }
        fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Value, E> {
            Ok(json!(v))
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Value, E> {
            Ok(json!(v))
        }
        fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<Value, E> {
            Ok(json!(v))
        }
        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut a: A,
        ) -> std::result::Result<Value, A::Error> {
            let mut v = Vec::new();
            while let Some(x) = a.next_element_seed(Unique)? {
                v.push(x);
            }
            Ok(Value::Array(v))
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut a: A,
        ) -> std::result::Result<Value, A::Error> {
            let mut v = serde_json::Map::new();
            while let Some(k) = a.next_key::<String>()? {
                if v.contains_key(&k) {
                    return Err(serde::de::Error::custom("Duplicate semantic JSON key"));
                }
                v.insert(k, a.next_value_seed(Unique)?);
            }
            Ok(Value::Object(v))
        }
    }
    use serde::de::DeserializeSeed;
    require(
        input.len() <= 8 * 1024 * 1024,
        "Semantic gate exceeds bound",
    )?;
    let mut d = serde_json::Deserializer::from_str(input);
    let value = Unique.deserialize(&mut d)?;
    d.end()?;
    Ok(value)
}
fn same_refs(actual: &Value, expected: &[Value]) -> Result<()> {
    let actual = array(actual)?;
    let set = actual
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    let wanted = expected
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    require(
        actual.len() == wanted.len() && set == wanted,
        "Missing/duplicate/unknown force references",
    )
}
pub(crate) fn verify_evidence(
    items: &Value,
    refs: &[Value],
    owned: &BTreeMap<String, Value>,
) -> Result<()> {
    let items = array(items)?;
    let item_refs = items
        .iter()
        .map(|e| e["force_ref"].clone())
        .collect::<Vec<_>>();
    same_refs(&json!(item_refs), refs)?;
    for e in items {
        let d = owned
            .get(text(&e["force_ref"]["documentType"])?)
            .ok_or_else(|| invalid("Evidence document unavailable"))?;
        require(
            e["document_id"] == d["id"]
                && e["revision"] == d["revision"]
                && e["sha256"] == d["sha256"],
            "Wrong evidence document revision/digest",
        )?;
        let quote = text(&e["quote"])?;
        require(
            quote.len() <= 8192 && text(&d["content"])?.contains(quote),
            "Evidence quote not found in exact source revision",
        )?;
    }
    Ok(())
}
pub fn evaluate_semantic(
    content: &str,
    finish: Option<&str>,
    owned: &BTreeMap<String, Value>,
) -> Result<Value> {
    require(
        finish.is_none_or(|f| f == "stop"),
        "Truncated semantic generation",
    )?;
    let p = semantic_profile()?;
    let model = &p["gates"][0];
    let report = parse_unique_json(content)?;
    let fields = report
        .as_object()
        .ok_or_else(|| invalid("Semantic gate must be an object"))?;
    let allowed = [
        "schema",
        "profile",
        "gate_type",
        "score",
        "reviewed_documents",
        "criteria",
        "consistency",
        "sections",
    ];
    require(
        fields.len() == allowed.len() && fields.keys().all(|k| allowed.contains(&k.as_str())),
        "Missing/unknown semantic gate fields",
    )?;
    require(
        report["schema"] == "qroc-semantic-gate/1"
            && report["profile"] == SEMANTIC_PROFILE
            && report["gate_type"] == "GATE_4",
        "Unknown semantic profile/gate/schema",
    )?;
    let mut reviewed = BTreeSet::new();
    for d in array(&report["reviewed_documents"])? {
        let kind = text(&d["document_type"])?;
        require(
            reviewed.insert(kind.to_string()),
            "Duplicate reviewed document",
        )?;
        let actual = owned
            .get(kind)
            .ok_or_else(|| invalid("Missing required reviewed document"))?;
        require(
            d["document_id"] == actual["id"]
                && d["revision"] == actual["revision"]
                && d["sha256"] == actual["sha256"],
            "Reviewed source revision/digest changed",
        )?;
    }
    require(
        reviewed == unique_strings(&model["reviewedDocuments"])? && owned.len() == reviewed.len(),
        "Missing/unknown reviewed source types",
    )?;
    let expected_sections = unique_strings(&model["outputContract"]["requiredSections"])?;
    let sections = report["sections"]
        .as_object()
        .ok_or_else(|| invalid("Missing required output sections"))?;
    require(
        sections.keys().cloned().collect::<BTreeSet<_>>() == expected_sections,
        "Missing/unknown required output sections",
    )?;
    for v in sections.values() {
        text(v)?;
    }
    let mut criteria = serde_json::Map::new();
    let mut total = 0;
    let mut floors = true;
    for c in array(&report["criteria"])? {
        let id = text(&c["id"])?;
        let source = array(&model["criteria"])?
            .iter()
            .find(|s| s["id"] == id)
            .ok_or_else(|| invalid("Unknown stable criterion ID"))?;
        require(!criteria.contains_key(id), "Duplicate stable criterion ID")?;
        let score = number(&c["score"])?;
        let max = number(&c["max"])?;
        require(
            max == number(&source["weight"])? && score <= max,
            "Criterion score/weight mismatch",
        )?;
        let refs = array(&source["derivedFromForces"])?;
        same_refs(&c["force_refs"], refs)?;
        verify_evidence(&c["evidence"], refs, owned)?;
        text(&c["feedback"])?;
        total += score;
        floors &= score * 100 >= max * 70;
        criteria.insert(
            id.into(),
            json!({"score":score,"max":max,"feedback":c["feedback"]}),
        );
    }
    require(
        criteria.len() == array(&model["criteria"])?.len(),
        "Missing required stable criterion",
    )?;
    let score = number(&report["score"])?;
    require(
        score <= 100 && score == total,
        "Semantic total score inconsistent with criteria",
    )?;
    let mut seen = BTreeSet::new();
    let mut consistent = true;
    for r in array(&report["consistency"])? {
        let id = text(&r["id"])?;
        let source = array(&model["consistencyRules"])?
            .iter()
            .find(|s| s["id"] == id)
            .ok_or_else(|| invalid("Unknown consistency rule ID"))?;
        require(
            seen.insert(id.to_string()),
            "Duplicate consistency rule assessment",
        )?;
        let refs = [source["left"].clone(), source["right"].clone()];
        same_refs(&r["force_refs"], &refs)?;
        verify_evidence(&r["evidence"], &refs, owned)?;
        text(&r["feedback"])?;
        let status = text(&r["status"])?;
        require(
            matches!(status, "supported" | "contradicted" | "insufficient"),
            "Unknown consistency assessment status",
        )?;
        consistent &= status == "supported";
    }
    require(
        seen.len() == array(&model["consistencyRules"])?.len(),
        "Missing required consistency assessment",
    )?;
    let passed = score >= number(&model["threshold"])? && floors && consistent;
    Ok(
        json!({"document_type":"GATE_4","status":if passed{"passed"}else{"failed"},"score":score,"criteria":criteria,"threshold":model["threshold"],"reason":if passed{"Source-backed criterion totals, required consistency assessments and exact owner evidence validated; human review remains separate."}else{"Threshold, criterion floor or cross-document consistency assessment failed."},"semantic":{"profile":SEMANTIC_PROFILE,"profile_sha256":profile_sha256(),"source_identity":semantic_sources(),"reviewed_documents":report["reviewed_documents"],"consistency":report["consistency"],"standing":"deterministic binding of a source-backed assessment; semantic judgment remains human"}}),
    )
}
pub fn semantic_model(kind: &str) -> Result<Option<Value>> {
    if !supports_semantic(kind) {
        return Ok(None);
    }
    let profile = semantic_profile()?;
    Ok(profile["documents"]
        .as_array()
        .unwrap()
        .iter()
        .chain(profile["gates"].as_array().unwrap())
        .find(|m| m["documentType"] == kind || m["gateType"] == kind)
        .cloned())
}
pub fn semantic_prompt(kind: &str) -> Result<Option<String>> {
    let Some(model) = semantic_model(kind)? else {
        return Ok(None);
    };
    let contract = if kind == "GATE_4" {
        "Return ONE JSON object, no fences or markdown. Versioned format lowering: the numeric JSON score fulfills the retained requiresScoreTag contract; do NOT emit a literal [SCORE] tag. The original source model remains unchanged. Fields: schema=qroc-semantic-gate/1, profile=retained-market-review/1, gate_type=GATE_4, score=sum of all criterion scores (max100), reviewed_documents=[{document_type,document_id,revision,sha256}] identifying the two exact supplied current revisions, criteria=[{id:exact source criterion ID,score,max:source weight,feedback,force_refs:exact derivedFromForces,evidence:[{force_ref,document_id,revision,sha256,quote:exact source text}]}], consistency=[{id:exact source rule ID,status:supported|contradicted|insufficient,feedback,force_refs:[left,right],evidence:exact source quotations for both force refs}], sections={each exact requiredSections key:nonempty assessment}. Include every criterion/rule/force reference exactly once. Missing evidence is insufficient, never invent scores, quotes or supported comparisons. Contradicted/insufficient rules block pass. Semantic judgment remains human."
    } else {
        "Answer the exact source-owned question using every declared force and required output section, check the stated failure modes and reader test. Use only the supplied permitted upstreams; label missing evidence and assumptions."
    };
    Ok(Some(format!(
        "Source-backed profile {} (sha256 {})\n{}\n{}",
        SEMANTIC_PROFILE,
        profile_sha256(),
        model,
        contract
    )))
}

/// Section 3.2 content inputs. Gate barriers are execution dependencies, not prompt inputs.
/// IDEA is the project brain dump; T401 owns its hardened standing.
pub fn product_inputs(kind: &str) -> Option<&'static [&'static str]> {
    Some(match kind {
        "STRATEGY_BRIEF" => &["IDEA"],
        "PRE_MORTEM" | "PRD" => &["STRATEGY_BRIEF"],
        "VALIDATION_PLAN" | "TECHNICAL_ARCHITECTURE" => &["PRD"],
        "GATE_1" => &[
            "STRATEGY_BRIEF",
            "PRE_MORTEM",
            "PRD",
            "VALIDATION_PLAN",
            "TECHNICAL_ARCHITECTURE",
        ],
        "DESIGN_ALTERNATIVES" | "ROADMAP" => &["PRD", "TECHNICAL_ARCHITECTURE"],
        "UI_UX_SPEC" => &["PRD", "DESIGN_ALTERNATIVES"],
        "IMPLEMENTATION_PLAN" => &["TECHNICAL_ARCHITECTURE", "ROADMAP"],
        "GATE_2" => &[
            "DESIGN_ALTERNATIVES",
            "ROADMAP",
            "UI_UX_SPEC",
            "IMPLEMENTATION_PLAN",
        ],
        "EPICS" => &["PRD", "IMPLEMENTATION_PLAN", "UI_UX_SPEC"],
        "SLICE_INDEX" => &["EPICS"],
        "GATE_3" => &["EPICS", "SLICE_INDEX"],
        "AGENT_DIRECTIVES" => &["SLICE_INDEX", "TECHNICAL_ARCHITECTURE"],
        "TASKS" => &["SLICE_INDEX", "EPICS"],
        "COMPONENT_CONTRACTS" => &["TECHNICAL_ARCHITECTURE", "SLICE_INDEX"],
        "PHOD_SPEC" => &["COMPONENT_CONTRACTS"],
        "VERIFICATION" => &["PHOD_SPEC"],
        "CONSISTENCY_JOIN" => &["PHOD_SPEC", "VERIFICATION"],
        _ => return None,
    })
}
/// Layout stages, independent of the old total document order.
pub fn product_column(kind: &str) -> Option<usize> {
    Some(match kind {
        "IDEA" => 0,
        "STRATEGY_BRIEF" => 1,
        "PRE_MORTEM" | "PRD" => 2,
        "VALIDATION_PLAN" | "TECHNICAL_ARCHITECTURE" => 3,
        "GATE_1" => 4,
        "DESIGN_ALTERNATIVES" | "ROADMAP" => 5,
        "UI_UX_SPEC" | "IMPLEMENTATION_PLAN" => 6,
        "GATE_2" => 7,
        "EPICS" => 8,
        "SLICE_INDEX" => 9,
        "GATE_3" => 10,
        "AGENT_DIRECTIVES" | "TASKS" | "COMPONENT_CONTRACTS" => 11,
        "PHOD_SPEC" => 12,
        "VERIFICATION" => 13,
        "CONSISTENCY_JOIN" => 14,
        _ => return None,
    })
}
/// Last gate is an execution barrier for its whole following stage.
pub fn product_barrier(kind: &str) -> Option<&'static str> {
    match product_column(kind)? {
        5..=7 => Some("GATE_1"),
        8..=10 => Some("GATE_2"),
        11.. => Some("GATE_3"),
        _ => None,
    }
}

#[cfg(test)]
mod product_graph_tests {
    use super::*;
    #[test]
    fn all_sixteen_product_content_lists_match_section_3_2() {
        let expected: [(&str, &[&str]); 16] = [
            ("STRATEGY_BRIEF", &["IDEA"]),
            ("PRE_MORTEM", &["STRATEGY_BRIEF"]),
            ("PRD", &["STRATEGY_BRIEF"]),
            ("VALIDATION_PLAN", &["PRD"]),
            ("TECHNICAL_ARCHITECTURE", &["PRD"]),
            (
                "GATE_1",
                &[
                    "STRATEGY_BRIEF",
                    "PRE_MORTEM",
                    "PRD",
                    "VALIDATION_PLAN",
                    "TECHNICAL_ARCHITECTURE",
                ],
            ),
            ("DESIGN_ALTERNATIVES", &["PRD", "TECHNICAL_ARCHITECTURE"]),
            ("UI_UX_SPEC", &["PRD", "DESIGN_ALTERNATIVES"]),
            ("ROADMAP", &["PRD", "TECHNICAL_ARCHITECTURE"]),
            (
                "IMPLEMENTATION_PLAN",
                &["TECHNICAL_ARCHITECTURE", "ROADMAP"],
            ),
            (
                "GATE_2",
                &[
                    "DESIGN_ALTERNATIVES",
                    "ROADMAP",
                    "UI_UX_SPEC",
                    "IMPLEMENTATION_PLAN",
                ],
            ),
            ("EPICS", &["PRD", "IMPLEMENTATION_PLAN", "UI_UX_SPEC"]),
            ("SLICE_INDEX", &["EPICS"]),
            ("GATE_3", &["EPICS", "SLICE_INDEX"]),
            (
                "AGENT_DIRECTIVES",
                &["SLICE_INDEX", "TECHNICAL_ARCHITECTURE"],
            ),
            ("TASKS", &["SLICE_INDEX", "EPICS"]),
        ];
        for (kind, inputs) in expected {
            assert_eq!(product_inputs(kind), Some(inputs));
            assert_eq!(
                entry(kind).unwrap()["input_policy"]["allowed_upstream"],
                json!(inputs)
            );
            for input in inputs {
                assert!(product_column(input).unwrap() < product_column(kind).unwrap());
            }
        }
        assert_eq!(product_barrier("ROADMAP"), Some("GATE_1"));
        assert_eq!(product_barrier("EPICS"), Some("GATE_2"));
        assert_eq!(product_barrier("TASKS"), Some("GATE_3"));
    }
    #[test]
    fn tail_contract_inputs_and_columns_are_exportable_without_admission() {
        for (kind, inputs) in [
            (
                "COMPONENT_CONTRACTS",
                vec!["TECHNICAL_ARCHITECTURE", "SLICE_INDEX"],
            ),
            ("PHOD_SPEC", vec!["COMPONENT_CONTRACTS"]),
            ("VERIFICATION", vec!["PHOD_SPEC"]),
            ("CONSISTENCY_JOIN", vec!["PHOD_SPEC", "VERIFICATION"]),
        ] {
            assert_eq!(product_inputs(kind).unwrap(), inputs);
            assert!(entry(kind).is_err());
            assert!(product_column(kind).is_some());
        }
    }
}
