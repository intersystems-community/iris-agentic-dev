Class MyApp.FormalSpec Extends %RegisteredObject
{

Method WithArgs(pName As %String = "hello", ByRef pRef As %Integer) As %Boolean
{
    Quit 1
}

ClassMethod WithOutput(Output pResult As %String) As %Status
{
    Set pResult = "done"
    Quit $$$OK
}

Method NoArgs() As %String
{
    Quit "ok"
}

}
