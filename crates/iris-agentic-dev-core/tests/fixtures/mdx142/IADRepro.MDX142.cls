/// Repro cases for the claims in PR 142's iris-mdx skill that disagree with what IRIS returns.
/// Each case runs the PR's own query, unchanged, against the Samples-BI cubes HoleFoods and Patients.
/// Load Samples-BI into a namespace, import this class there, then:
///   Do ##class(IADRepro.MDX142).Run()        all cases
///   Do ##class(IADRepro.MDX142).Run("R3")    one case
/// Each case prints the claim, the query, the stage that failed (prepare or execute) with its
/// error text, or the grid with its row and column labels.
Class IADRepro.MDX142
{

/// Case list: $LB(claim, query). Cases with two queries use R<n>a and R<n>b.
ClassMethod Cases(Output pCases) As %Status
{
	Set tSC = $$$OK
	Set pCases("R1a") = $ListBuild("%MDX() directly on an axis returns empty, no error (checklist, section 11, section 13). The PR's query as written; empty on every run after the first", "SELECT %MDX(""SELECT MEASURES.[Amount Sold] ON 0 FROM HoleFoods"") ON 0 FROM HoleFoods")
	Set pCases("R1b") = $ListBuild("Not a PR claim. R1a with a text no one has run: run 1 has a value, run 2 of the same text is empty. Use Twice()", "SELECT %MDX(""WITH MEMBER MEASURES.[N%NONCE%] AS '1' SELECT MEASURES.[Amount Sold] ON 0 FROM HoleFoods"") ON 0 FROM HoleFoods")
	Set pCases("R1c") = $ListBuild("Not a PR claim. Same, subquery on Patients, outer query on HoleFoods. Use Twice()", "SELECT %MDX(""WITH MEMBER MEASURES.[N%NONCE%] AS '1' SELECT MEASURES.[%COUNT] ON 0 FROM Patients"") ON 0 FROM HoleFoods")
	Set pCases("R1d") = $ListBuild("Not a PR claim. Control: the subquery run as plain MDX twice. Use Twice()", "WITH MEMBER MEASURES.[N%NONCE%] AS '1' SELECT MEASURES.[Amount Sold] ON 0 FROM HoleFoods")
	Set pCases("R2") = $ListBuild("MEASURES.MEMBERS excludes %COUNT (section 9)", "SELECT MEASURES.MEMBERS ON 0 FROM HoleFoods")
	Set pCases("R3") = $ListBuild("MAX(set, measure) on the same axis as the members avoids 'Measures cannot exist on multiple axes' (section 11)", "SELECT MEASURES.[%COUNT] ON 0, NON EMPTY {[DiagD].[H1].[Diagnoses].MEMBERS, MAX([DiagD].[H1].[Diagnoses].MEMBERS, MEASURES.[%COUNT])} ON 1 FROM Patients")
	Set pCases("R4") = $ListBuild("Without %LABEL, a PrevMember measure's header shows the dimension name DateOfSale (section 10)", "WITH MEMBER MEASURES.[PrevUnits] AS '([DateOfSale].[Actual].CurrentMember.PrevMember, MEASURES.[Units Sold])' SELECT {MEASURES.[Units Sold], MEASURES.[PrevUnits]} ON 0, NON EMPTY [DateOfSale].[Actual].[MonthSold].MEMBERS ON 1 FROM HoleFoods")
	Set pCases("R5a") = $ListBuild("WHERE {a, b} can double-count; %OR is the correct union (section 7). WHERE form", "SELECT MEASURES.[%COUNT] ON 0 FROM Patients WHERE {[DiagD].[H1].[Diagnoses].&[asthma], [DiagD].[H1].[Diagnoses].&[diabetes]}")
	Set pCases("R5b") = $ListBuild("WHERE {a, b} can double-count; %OR is the correct union (section 7). %OR form", "SELECT MEASURES.[%COUNT] ON 0 FROM Patients %FILTER %OR({[DiagD].[H1].[Diagnoses].&[asthma], [DiagD].[H1].[Diagnoses].&[diabetes]})")
	Set pCases("R6") = $ListBuild("%OR on an axis returns one row labelled ""asthma+"" (section 7)", "SELECT {MEASURES.[%COUNT], MEASURES.[Avg Age]} ON 0, NON EMPTY %OR({[DiagD].[H1].[Diagnoses].&[asthma], [DiagD].[H1].[Diagnoses].&[diabetes]}) ON 1 FROM Patients")
	Set pCases("R7a") = $ListBuild("Channel is integer-keyed: &[2] is CORRECT for Online, &[Online] returns null (section 8). &[2]", "SELECT MEASURES.[%COUNT] ON 0 FROM HoleFoods WHERE [Channel].[H1].[Channel Name].&[2]")
	Set pCases("R7b") = $ListBuild("Channel is integer-keyed: &[2] is CORRECT for Online, &[Online] returns null (section 8). &[Online]", "SELECT MEASURES.[%COUNT] ON 0 FROM HoleFoods WHERE [Channel].[H1].[Channel Name].&[Online]")
	Set pCases("R7c") = $ListBuild("Discover keys with CURRENTMEMBER.PROPERTIES(""KEY"") (section 8)", "SELECT [Channel].[H1].CURRENTMEMBER.PROPERTIES(""KEY"") ON 0, [Channel].[H1].[Channel Name].MEMBERS ON 1 FROM HoleFoods")
	Set pCases("R8a") = $ListBuild("A Patients dimension in a HoleFoods query gives ERROR #5001: Invalid Member spec (section 13). On an axis", "SELECT MEASURES.[%COUNT] ON 0, [GenD].[H1].[Gender].MEMBERS ON 1 FROM HoleFoods")
	Set pCases("R8b") = $ListBuild("A Patients dimension in a HoleFoods query gives ERROR #5001: Invalid Member spec (section 13). In %FILTER", "SELECT MEASURES.[%COUNT] ON 0 FROM HoleFoods %FILTER [GenD].[H1].[Gender].&[Female]")
	Set pCases("R9") = $ListBuild("Nonexistent measure gives ERROR #5001: Measure not found (section 13). Which stage", "SELECT MEASURES.[Nope] ON 0 FROM HoleFoods")
	Set pCases("R10a") = $ListBuild("Not a PR claim. Compound NOW offset on a month level. [NOW-1y10m]", "SELECT MEASURES.[%COUNT] ON 0, [DateOfSale].[Actual].[MonthSold].[NOW-1y10m] ON 1 FROM HoleFoods")
	Set pCases("R10b") = $ListBuild("Not a PR claim. The same offset as one number. [NOW-22m]", "SELECT MEASURES.[%COUNT] ON 0, [DateOfSale].[Actual].[MonthSold].[NOW-22m] ON 1 FROM HoleFoods")
	Set pCases("R10c") = $ListBuild("Not a PR claim. One month back, for comparison with R10a. [NOW-1m]", "SELECT MEASURES.[%COUNT] ON 0, [DateOfSale].[Actual].[MonthSold].[NOW-1m] ON 1 FROM HoleFoods")
	Quit tSC
}

/// Run one case (pId) or every case, printing each outcome.
ClassMethod Run(pId As %String = "") As %Status
{
	Set tSC = $$$OK
	Set tSC = ..Cases(.tCases)
	If $$$ISERR(tSC) Quit tSC
	Set tId = ""
	For {
		Set tId = $Order(tCases(tId))
		Quit:tId=""
		Continue:(pId'="")&&(tId'=pId)&&($Extract(tId,1,$Length(pId))'=pId)
		Set tCase = tCases(tId)
		Write "=== ", tId, " ===", !
		Write "Claim: ", $ListGet(tCase, 1), !
		Write "Query: ", $ListGet(tCase, 2), !
		Set tQuery = $ListGet(tCase, 2)
		Write "Result: ", $Select(tQuery["%NONCE%":..Twice(tQuery), 1:..Outcome(tQuery)), !!
	}
	Quit tSC
}

/// Run a case twice, with %NONCE% replaced by a value unique to this call so run 1 has no
/// results cache entry. Returns "1: <outcome> || 2: <outcome>".
ClassMethod Twice(pQuery As %String) As %String
{
	Set tQuery = $Replace(pQuery, "%NONCE%", $Job_$Translate($ZTimeStamp, ",."))
	Quit "1: "_..Outcome(tQuery)_" || 2: "_..Outcome(tQuery)
}

/// One line: PREPARE ERROR <text>, EXECUTE ERROR <text>, or the grid as
/// cols=[label|label] then row label: cell|cell; ...
ClassMethod Outcome(pQuery As %String) As %String
{
	Set tSC = $$$OK
	Set tRS = ##class(%DeepSee.ResultSet).%New()
	Set tSC = tRS.%PrepareMDX(pQuery)
	If $$$ISERR(tSC) Quit "PREPARE ERROR "_$System.Status.GetErrorText(tSC)
	Set tSC = tRS.%Execute()
	If $$$ISERR(tSC) Quit "EXECUTE ERROR "_$System.Status.GetErrorText(tSC)
	Set tCols = tRS.%GetAxisSize(1)
	Set tRows = tRS.%GetAxisSize(2)
	Set tOut = "cols=["
	For tC = 1:1:tCols {
		Kill tLab
		Set tN = tRS.%GetOrdinalLabel(.tLab, 1, tC)
		Set tOut = tOut_$Select(tC>1:"|", 1:"")_$Get(tLab(1))
	}
	Set tOut = tOut_"] rows="_tRows
	If tRows = 0 {
		Set tLine = ""
		For tC = 1:1:tCols Set tLine = tLine_$Select(tC>1:"|", 1:"")_tRS.%GetOrdinalValue(tC)
		Quit tOut_" cells: "_tLine
	}
	For tR = 1:1:tRows {
		Kill tLab
		Set tN = tRS.%GetOrdinalLabel(.tLab, 2, tR)
		Set tLine = ""
		For tC = 1:1:$Select(tCols>0:tCols, 1:1) Set tLine = tLine_$Select(tC>1:"|", 1:"")_tRS.%GetOrdinalValue(tC, tR)
		Set tOut = tOut_"; "_$Get(tLab(1))_": "_tLine
	}
	Quit tOut
}

}
