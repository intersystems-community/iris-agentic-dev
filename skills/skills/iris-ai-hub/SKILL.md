---
name: iris-ai-hub
author: tdyar
version: 0.1.0
managed_by: iris-agentic-dev
description: "IRIS AI Hub production patterns — %AI.Agent, %AI.Tool, %AI.ToolSet, Ens.BusinessOperation wrapping agents, BPL async suspend, EnsLib.Workflow.Operation human-in-the-loop. Load when building AI Hub integrations."
---

# iris-ai-hub

## Availability — EAP only

**IRIS AI Hub (`%AI.*` classes) is not in any generally available IRIS release.**
It is only available through the Early Access Program (EAP) as downloadable container
images from [evaluation.intersystems.com](https://evaluation.intersystems.com).

- Do NOT suggest `zpm install`, `pip install`, or any package manager path — none exist yet.
- Do NOT suggest downloading from Docker Hub or GHCR — the images are gated behind EAP login.
- If the user does not have an EAP image, direct them to evaluation.intersystems.com to request access.
- All `%AI.*` classes shown below are confirmed against EAP container builds. They may change before GA.

## When to load this skill

Load when the user is working with an EAP IRIS AI Hub container and building:

- `%AI.Agent` or `%AI.Tool` / `%AI.ToolSet` classes
- Interoperability productions that invoke AI agents as Business Operations
- Human-in-the-loop workflows combining AI assessment with `EnsLib.Workflow.Operation`
- Python-vs-ObjectScript architecture decisions for AI Hub integrations

---

## Python vs ObjectScript — choose first

Before writing any code, establish where the orchestration lives:

| Scenario                                                           | Right choice                                                |
| ------------------------------------------------------------------ | ----------------------------------------------------------- |
| External Python script or embedded Python BO calling an agent      | `iris_llm.Agent` (Python)                                   |
| Agent must run as a native Interoperability BO inside a production | `Ens.BusinessOperation` wrapping `%AI.Agent` (ObjectScript) |
| Defining production structure (items, settings)                    | `pyprod` (Python) or production XML — either works          |

These are distinct patterns. Do not wrap `iris_llm.Agent` inside a BO when a direct Python path exists, and do not reach for ObjectScript when the caller is already Python.

---

## `%AI.Agent` — ObjectScript inside IRIS

```objectscript
Set settings = {}
Do settings.%Set("api_key", $System.Util.GetEnviron("OPENAI_API_KEY"))
Set provider = ##class(%AI.Provider).Create("openai", settings)  // or "anthropic"

Set agent = ##class(%AI.Agent).%New(provider)  // provider arg optional; set later via ..Provider
Set agent.Model = "gpt-4o-mini"
Set agent.SystemPrompt = "You are a clinical assessment assistant."
Do agent.%Init()

Set session = agent.CreateSession()
Set resp = agent.Chat(session, "Assess patient P001 for urgency")
Write resp.Content  // %AI.LLM.Response has: Content, ToolCalls, Usage
```

Key facts:

- `%AI.Agent.%New(provider)` — provider is optional; assign `agent.Provider` before `%Init()` if deferred.
- `%Init()` must be called before `Chat()` or `CreateSession()`.
- `Chat()` returns `%AI.LLM.Response` — read `resp.Content` for the text result.
- Add toolsets via `agent.UseToolSet("My.ToolSet.Class")` or `agent.ToolManager.AddTool(obj)`.
- `%AI.AgentOperation` does NOT exist — there is no base class for "agent as BO". Write a plain `Ens.BusinessOperation` that instantiates `%AI.Agent` inside the handler method.

---

## `%AI.Tool` — single tool class (simplest)

Extend `%AI.Tool`. Every public `ClassMethod` becomes a tool automatically. Parameter types and
descriptions are inferred from the method signature and doc comments above each method.
No XData needed.

```objectscript
Class EHR.Tools.PatientTool Extends %AI.Tool
{
/// Get clinical summary for a patient.
/// patientId: the patient identifier
ClassMethod GetPatientSummary(patientId As %String) As %String
{
    Quit "Summary for patient: "_patientId
}
}
```

---

## `%AI.ToolSet` — multiple tools in one class

Extend `%AI.ToolSet`. Define tools via `XData Definition [MimeType = application/xml]`.
Each `<Tool Name= Method=>` entry maps to a ClassMethod of the same name.
Parameter types are inferred from the method signature — do NOT add a `<Parameters>` block
(causes `ERROR #6237: Unexpected tag` on current EAP builds).

```objectscript
Class EHR.Tools.PatientToolSet Extends %AI.ToolSet
{

XData Definition [ MimeType = application/xml ]
{
<ToolSet Name="PatientTools">
  <Description>Patient data tools for AI assessment</Description>
  <Tool Name="GetPatientSummary" Method="GetPatientSummary">
    <Description>Get clinical summary for a patient. Pass patientId as the patient identifier.</Description>
  </Tool>
  <Tool Name="GetMedications" Method="GetMedications">
    <Description>List current medications for a patient. Pass patientId.</Description>
  </Tool>
</ToolSet>
}

ClassMethod GetPatientSummary(patientId As %String) As %String
{
    Quit "Patient "_patientId_": stable"
}

ClassMethod GetMedications(patientId As %String) As %String
{
    Quit "Medications for "_patientId_": none on file"
}

}
```

---

## Agent as Interoperability BO

`%AI.AgentOperation` does not exist. The correct pattern is a plain `Ens.BusinessOperation`
that instantiates `%AI.Agent` inside the handler method:

```objectscript
Class EHR.Workflow.AssessmentAgentOperation Extends Ens.BusinessOperation
{

Parameter ADAPTER = "Ens.OutboundAdapter";
Parameter INVOCATION = "Queue";

Method HandleAssessment(pRequest As EHR.Workflow.AssessmentRequest,
    Output pResponse As EHR.Workflow.AssessmentResponse) As %Status
{
    Set pResponse = ##class(EHR.Workflow.AssessmentResponse).%New()
    Set apiKey = $System.Util.GetEnviron("OPENAI_API_KEY")
    If apiKey = "" { Return $$$ERROR($$$GeneralError, "OPENAI_API_KEY not set") }

    Set settings = {}
    Do settings.%Set("api_key", apiKey)
    Set provider = ##class(%AI.Provider).Create("openai", settings)
    Set agent = ##class(%AI.Agent).%New(provider)
    Set agent.Model = "gpt-4o-mini"
    Set agent.SystemPrompt = "You are a clinical assessment agent."
    Do agent.%Init()

    Set session = agent.CreateSession()
    Set resp = agent.Chat(session, "Patient: "_pRequest.PatientId_" Notes: "_pRequest.Notes)
    Set pResponse.Recommendation = resp.Content
    Return $$$OK
}

XData MessageMap
{
<MapItems>
  <MapItem MessageType="EHR.Workflow.AssessmentRequest">
    <Method>HandleAssessment</Method>
  </MapItem>
</MapItems>
}

}
```

New BOs must have `Enabled="false"` in production XML. Never invent placeholder IP/port/URL settings.

---

## BPL async suspend with agent BO + human workflow

Use `<call async='true'>` + `<sync>` — never `SendRequestSync` inside a `<code>` block
(`SendRequestSync` does not exist in the BPL thread context; it is a compile error).
Never use `initialExpression` on a `<context>` `<property>` — use `<assign>` instead
(causes `ERROR <Ens>ErrInvalidBPL` on compile).

```objectscript
Class EHR.Workflow.PatientUnwellProcessBPL Extends Ens.BusinessProcessBPL
{

XData BPL [ XMLNamespace = "http://www.intersystems.com/bpl" ]
{
<process language='objectscript'
    request='EHR.Workflow.PatientUnwellMessage'
    response='Ens.Response'>
<context>
  <property name='agentResponse' type='EHR.Workflow.AssessmentResponse' instantiate='0'/>
</context>
<sequence>

  <call name='CallAssessmentAgent' target='AssessmentAgentOperation' async='true'>
    <request type='EHR.Workflow.AssessmentRequest'>
      <assign property='callrequest.PatientId' value='request.PatientId' action='set'/>
      <assign property='callrequest.Notes' value='request.Notes' action='set'/>
    </request>
    <response type='EHR.Workflow.AssessmentResponse'>
      <assign property='context.agentResponse' value='callresponse' action='set'/>
    </response>
  </call>

  <sync name='WaitForAssessment' calls='CallAssessmentAgent' type='all'/>

  <call name='PhysicianReview' target='PhysicianReviewOperation' async='true'>
    <request type='EnsLib.Workflow.TaskRequest'>
      <assign property='callrequest.%Subject'
              value='"Approve medication for "_request.PatientId' action='set'/>
      <assign property='callrequest.%Actions' value='"Approve,Reject"' action='set'/>
      <assign property='callrequest.%Priority'
              value='context.agentResponse.UrgencyLevel' action='set'/>
    </request>
  </call>

  <sync name='WaitForPhysician' calls='PhysicianReview' type='all'/>

</sequence>
</process>
}

}
```

---

## Message classes

```objectscript
Class EHR.Workflow.AssessmentRequest Extends Ens.Request
{
Property PatientId As %String(MAXLEN = 50);
Property Notes As %String(MAXLEN = 32000);  // NOT bare %String — default MAXLEN=50 truncates
}

Class EHR.Workflow.AssessmentResponse Extends Ens.Response
{
Property Recommendation As %String(MAXLEN = 32000);
Property Medication As %String;
Property UrgencyLevel As %Integer;  // NOT %String or %Numeric
}
```

- Always use `Extends Ens.Request` / `Extends Ens.Response` — never `%Persistent` directly.
- Long text fields need explicit `MAXLEN` — the default 50 will silently truncate AI output.
- Urgency/severity levels are `%Integer`, not `%String`.
