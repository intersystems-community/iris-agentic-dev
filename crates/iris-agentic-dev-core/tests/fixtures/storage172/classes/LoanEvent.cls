Class IadSm172.LoanEvent Extends %Persistent [ StorageStrategy = SQLStorage ]
{

Property Loan As %Integer;

Property EvNo As %Integer;

Property Action As %String;

Property EvDate As %Date;

Index IDKEY On (Loan, EvNo) [ IdKey, Unique ];

Storage SQLStorage
{
<SQLMap name="Master">
<Data name="Action">
<Delimiter>"^"</Delimiter>
<Piece>1</Piece>
</Data>
<Data name="EvDate">
<Delimiter>"^"</Delimiter>
<Piece>2</Piece>
</Data>
<Global>^IadSm172Loan</Global>
<Subscript name="1">
<Expression>{Loan}</Expression>
</Subscript>
<Subscript name="2">
<Expression>"EV"</Expression>
</Subscript>
<Subscript name="3">
<Expression>{EvNo}</Expression>
</Subscript>
<Type>data</Type>
</SQLMap>
<StreamLocation>^IadSm172LoanEventS</StreamLocation>
<Type>%Storage.SQL</Type>
}

}

