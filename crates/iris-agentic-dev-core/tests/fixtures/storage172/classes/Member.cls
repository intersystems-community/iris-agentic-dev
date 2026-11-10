Class IadSm172.Member Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Member)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Age">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Data name="City">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Member</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172MemberS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

