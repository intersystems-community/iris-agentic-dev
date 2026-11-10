Class IadSm172.LoanList Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property MemberId As %Integer;

Property BookIds As list Of %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172LoanList)</SqlIdExpression>
<SQLMap name="Master">
<Data name="BookIds"/>
<Data name="MemberId">
<Piece>1</Piece>
</Data>
<Global>^IadSm172LoanList</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172LoanListS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

