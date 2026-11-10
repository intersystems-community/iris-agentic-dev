Class IadSm172.LoanUnion Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property MemberId As %Integer;

Property BookId As %String;

Property LoanNo As %Integer;

Index IDKEY On (MemberId, LoanNo) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="BookId">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172LoanUIdx</Global>
<RowReference>^IadSm172LoanU({L2})</RowReference>
<Subscript name="1">
<Expression>{MemberId}</Expression>
</Subscript>
<Subscript name="2">
<Expression>{LoanNo}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172LoanUS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

