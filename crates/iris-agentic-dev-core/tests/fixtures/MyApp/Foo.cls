Class MyApp.Foo Extends %RegisteredObject
{

Parameter VERSION = 1;

Property Value As %String;

ClassMethod DoSomething(name As %String) As %Boolean
{
    Quit 1
}

ClassMethod DoOther() As %String
{
    Quit "other"
}

Method Helper(x As %Integer, y As %Integer) As %Integer
{
    Quit x + y
}

}
