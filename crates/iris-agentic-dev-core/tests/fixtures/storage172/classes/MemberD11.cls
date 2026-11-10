Class IadSm172.MemberD11 Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Name As %String;

Property City As %String;

Property Email As %String;

Property Phone As %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172MemberD)</SqlIdExpression>
<SQLMap name="Master">
<Data name="City">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="Email">
<Delimiter>"^"</Delimiter>
<Node>"contact"</Node>
<Piece>1</Piece>
</Data>
<Data name="Name">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Phone">
<Delimiter>"^"</Delimiter>
<Node>"contact"</Node>
<Piece>2</Piece>
</Data>
<Global>^IadSm172MemberD</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172MemberDS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

