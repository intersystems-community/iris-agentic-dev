Class IadSm172.Loan Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property MemberId As %Integer;

Property BookId As %String;

Property LoanDate As %Date;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172Loan)</SqlIdExpression>
<SQLMap name="Master">
<Data name="BookId">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="LoanDate">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Data name="MemberId">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Global>^IadSm172Loan</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172LoanS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

