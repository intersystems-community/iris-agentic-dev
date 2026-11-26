Class IadSm173.F43B Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Label As %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm173F43Bc)</SqlIdExpression>
<SQLMap name="Master">
<Data name="Label">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^IadSm173F43X</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm173F43Bs</StreamLocation>
<Type>%Storage.SQL</Type>
}

}
