Class IadSm172.Shipment Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Dest As %String;

Property Status As %String;

Index OpenByDest On Dest [ Condition = {Status="open"} ];

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Ship)</SqlIdExpression>
<SQLMap name="Master">
<BlockCount>-4</BlockCount>
<Data name="Dest">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Status">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Ship</Global>
<PopulationPct>100</PopulationPct>
<Structure>delimited</Structure>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="OpenByDest">
<Condition>{Status}="open"</Condition>
<ConditionalWithHostVars>1</ConditionalWithHostVars>
<ConditionFields>Status</ConditionFields>
<Global>^IadSm172ShipI</Global>
<PopulationType>conditional</PopulationType>
<Subscript name="1">
<Expression>{Dest}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<StreamLocation>^IadSm172ShipS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
