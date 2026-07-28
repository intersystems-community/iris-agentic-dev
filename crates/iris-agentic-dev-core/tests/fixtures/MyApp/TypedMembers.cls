Class MyApp.TypedMembers Extends %RegisteredObject
{

Property Value As %String;

Property Count As %Integer;

ClassMethod DoSomething(name As %String) As %Boolean
{
    Quit 1
}

Method Helper(x As %Integer) As %Integer
{
    Quit x + 1
}

Parameter VERSION = 1;

}
