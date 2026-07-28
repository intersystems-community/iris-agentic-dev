Class MyApp.AllMembers Extends (%Persistent, %Populate)
{

Property Name As %String;

Index ByName On Name;

Query AllItems() As %SQLQuery
{
    SELECT Name FROM MyApp.AllMembers
}

Trigger OnInsert After Insert
{
}

XData DefaultData
{
}

Relationship Parent As MyApp.AllMembers [ Cardinality = many, Inverse = Children ];

ForeignKey FKName(Name) References MyApp.AllMembers(Name);

Projection ProjectionDef As %Projection.SQL;

Storage Default
{
<Data name="AllMembersDefaultData">
<Value name="1">
<Value>%%CLASSNAME</Value>
</Value>
</Data>
<DataLocation>^MyApp.AllMembersD</DataLocation>
<DefaultData>AllMembersDefaultData</DefaultData>
<IdLocation>^MyApp.AllMembersD</IdLocation>
<IndexLocation>^MyApp.AllMembersI</IndexLocation>
<StreamLocation>^MyApp.AllMembersS</StreamLocation>
<Type>%Storage.Persistent</Type>
}

}
