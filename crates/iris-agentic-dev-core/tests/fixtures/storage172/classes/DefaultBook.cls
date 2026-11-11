Class IadSm172.DefaultBook Extends %Persistent
{

Property Title As %String;

Property Author As %String;

Property ISBN As %String;

Storage Default
{
<Data name="DefaultBookDefaultData">
<Value name="1">
<Value>%%CLASSNAME</Value>
</Value>
<Value name="2">
<Value>Title</Value>
</Value>
<Value name="3">
<Value>Author</Value>
</Value>
<Value name="4">
<Value>ISBN</Value>
</Value>
</Data>
<DataLocation>^IadSm172.DefaultBookD</DataLocation>
<DefaultData>DefaultBookDefaultData</DefaultData>
<IdLocation>^IadSm172.DefaultBookD</IdLocation>
<IndexLocation>^IadSm172.DefaultBookI</IndexLocation>
<StreamLocation>^IadSm172.DefaultBookS</StreamLocation>
<Type>%Storage.Persistent</Type>
}

}

