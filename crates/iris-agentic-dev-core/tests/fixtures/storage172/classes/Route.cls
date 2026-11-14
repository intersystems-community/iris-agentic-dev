Class IadSm172.Route Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Depot As %String;

Property Seq As %Integer;

Property Name As %String;

Property Stops As %Integer;

Index IDKEY On (Depot, Seq) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Stops">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Route</Global>
<Subscript name="1">
<AccessType>sub</AccessType>
<Accessvar name="1">
<Code> s {1D1}=$g(^IadSm172Route({L1}))</Code>
<Variable>{1D1}</Variable>
</Accessvar>
<Expression>{Depot}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{Seq}</Expression>
<Invalidcondition name="1">
<Expression>'{1D1}</Expression>
</Invalidcondition>
<StopExpression>{L2}\900</StopExpression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172RouteS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
