Class IadSm172.ActiveLoan Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property MemberId As %Integer;

Property BookId As %String;

Property Status As %String;

Storage SQLStorage
{
<SqlIdExpression>$i(^IadSm172ActiveLoan)</SqlIdExpression>
<SQLMap name="Master">
<Data name="BookId">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Data name="MemberId">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="Status">
<Delimiter>"^"</Delimiter>
<Piece>3</Piece>
</Data>
<Global>^IadSm172ActiveLoan</Global>
<Subscript name="1">
<Expression>{ID}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<SQLMap name="StatusIndex">
<Condition>{Status} = "active"</Condition>
<Global>^IadSm172ActiveLoanIdx</Global>
<PopulationType>conditional</PopulationType>
<Subscript name="1">
<Expression>{Status}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{ID}</Expression>
</Subscript>
<Type>index</Type>
</SQLMap>
<StreamLocation>^IadSm172ActiveLoanS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

