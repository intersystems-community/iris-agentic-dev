Class MyApp.PythonBody Extends %RegisteredObject
{

ClassMethod Greet(name As %String) As %String [ Language = python ]
{
    return "Hello, " + name
}

Method Compute(x As %Integer) As %Integer [ Language = python ]
{
    return x * 2
}

}
