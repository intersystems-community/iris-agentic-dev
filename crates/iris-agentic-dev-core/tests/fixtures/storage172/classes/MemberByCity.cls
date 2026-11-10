Class IadSm172.MemberByCity Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Name As %String;

Property City As %String;

Property Age As %Integer;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172MemberByCity)</SqlIdExpression>
<SQLMap name="CityIdx">
<Global>^IadSm172MemberByCityI</Global>
<Subscript name="1">
<Expression>{City}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
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
<Global>^IadSm172MemberByCity</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172MemberByCityS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

